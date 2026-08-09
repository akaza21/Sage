mod engine;
mod file_scan;
mod types;

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use tauri::State;
use tokio::{sync::Semaphore, time::timeout};

use crate::{
    error::{AppError, AppResult},
    persistence::Database,
    state::AppState,
    workspace::Workspace,
};
use engine::SearchEngine;
use types::{SearchLimits, SearchRoot, SearchRootSource};
pub use types::{WorkspaceSearchRequest, WorkspaceSearchResponse};

const MAX_PROJECT_ID_BYTES: usize = 512;
const SEARCH_QUEUE_WAIT: Duration = Duration::from_millis(250);
const SEARCH_COMMAND_TIMEOUT: Duration = Duration::from_millis(2_500);
static SEARCH_PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[tauri::command]
pub async fn search_workspace(
    state: State<'_, AppState>,
    request: WorkspaceSearchRequest,
) -> Result<WorkspaceSearchResponse, String> {
    let permit = timeout(SEARCH_QUEUE_WAIT, search_permits().acquire_owned())
        .await
        .map_err(|_| "workspace search is busy; retry after the active scan finishes".to_owned())?
        .map_err(|_| "workspace search is unavailable".to_owned())?;
    let database = state.database.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        search_active_project(&database, request, SearchLimits::default())
    });
    timeout(SEARCH_COMMAND_TIMEOUT, task)
        .await
        .map_err(|_| "workspace search exceeded its 2500 ms command deadline".to_owned())?
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

fn search_permits() -> Arc<Semaphore> {
    SEARCH_PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn search_active_project(
    database: &Database,
    request: WorkspaceSearchRequest,
    limits: SearchLimits,
) -> AppResult<WorkspaceSearchResponse> {
    if request.project_id.is_empty()
        || request.project_id.len() > MAX_PROJECT_ID_BYTES
        || request.project_id.contains('\0')
    {
        return Err(AppError::InvalidRequest(
            "workspace search project ID is invalid".into(),
        ));
    }
    if database.active_project_id()?.as_deref() != Some(&request.project_id) {
        return Err(AppError::InvalidRequest(
            "workspace search project is not active".into(),
        ));
    }

    let summary = database.project_summary(&request.project_id)?;
    let root_names = summary
        .roots
        .into_iter()
        .map(|root| (root.id, root.name))
        .collect::<HashMap<_, _>>();
    let roots = database
        .root_records(&request.project_id)?
        .into_iter()
        .map(|root| {
            let root_name = root_names
                .get(&root.id)
                .cloned()
                .unwrap_or_else(|| "Root".into());
            let source = match Workspace::open(&root.project_id, &root.id, &root.absolute_path) {
                Ok(workspace) => SearchRootSource::Available(workspace),
                Err(error) => SearchRootSource::Unavailable(error.to_string()),
            };
            SearchRoot {
                root_id: root.id,
                root_name,
                source,
            }
        })
        .collect();
    SearchEngine::new(limits).search(request, roots)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use uuid::Uuid;

    use super::{
        engine::SearchEngine,
        types::{SearchLimits, SearchRoot, SearchRootSource, WorkspaceSearchRequest},
    };
    use crate::{persistence::Database, workspace::Workspace};

    fn fixture_root() -> (PathBuf, SearchRoot) {
        let path = std::env::temp_dir().join(format!("sage-search-{}", Uuid::new_v4()));
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(
            path.join("src").join("main.rs"),
            "fn main() {\n    let greeting = \"Hello Sage\";\n}\n",
        )
        .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        (
            path,
            SearchRoot {
                root_id: "root-test".into(),
                root_name: "fixture".into(),
                source: SearchRootSource::Available(workspace),
            },
        )
    }

    #[test]
    fn finds_a_literal_with_a_canonical_resource_identity() {
        let (path, root) = fixture_root();
        let result = SearchEngine::new(SearchLimits::default())
            .search(
                WorkspaceSearchRequest {
                    project_id: "project-test".into(),
                    query: "hello sage".into(),
                    case_sensitive: false,
                },
                vec![root],
            )
            .unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].resource.root_id, "root-test");
        assert_eq!(result.matches[0].resource.relative_path, "src/main.rs");
        assert_eq!(result.matches[0].line_number, 2);
        assert_eq!(result.matches[0].column_number, 21);
        assert_eq!(result.matches[0].preview, "let greeting = \"Hello Sage\";");
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn active_project_search_keeps_missing_roots_local_to_their_report() {
        let missing = std::env::temp_dir().join(format!("sage-search-{}", Uuid::new_v4()));
        let available = std::env::temp_dir().join(format!("sage-search-{}", Uuid::new_v4()));
        fs::create_dir_all(&missing).unwrap();
        fs::create_dir_all(&available).unwrap();
        fs::write(missing.join("gone.txt"), "needle in missing root\n").unwrap();
        fs::write(available.join("kept.txt"), "needle in available root\n").unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&missing).unwrap();
        database.add_project_root(&project.id, &available).unwrap();
        fs::remove_dir_all(&missing).unwrap();

        let result = super::search_active_project(
            &database,
            WorkspaceSearchRequest {
                project_id: project.id,
                query: "needle".into(),
                case_sensitive: false,
            },
            SearchLimits::default(),
        )
        .unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].resource.relative_path, "kept.txt");
        assert_eq!(result.roots.len(), 2);
        assert_eq!(
            result.roots[0].state,
            super::types::SearchRootState::Unavailable
        );
        assert_eq!(
            result.roots[1].state,
            super::types::SearchRootState::Complete
        );
        assert!(
            result.roots[0]
                .message
                .as_deref()
                .unwrap()
                .contains("filesystem")
        );
        fs::remove_dir_all(available).unwrap();
    }

    #[test]
    fn oversized_files_are_skipped_without_crossing_the_per_file_bound() {
        let (path, root) = fixture_root();
        fs::write(path.join("large.txt"), "needle is beyond the bound").unwrap();
        let limits = SearchLimits {
            max_file_bytes: 8,
            ..SearchLimits::default()
        };

        let result = SearchEngine::new(limits)
            .search(
                WorkspaceSearchRequest {
                    project_id: "project-test".into(),
                    query: "needle".into(),
                    case_sensitive: false,
                },
                vec![root],
            )
            .unwrap();

        assert!(result.matches.is_empty());
        assert_eq!(result.roots[0].files_scanned, 0);
        assert_eq!(result.roots[0].skipped_files, 2);
        assert_eq!(
            result.limit_reasons,
            vec![super::types::SearchLimitReason::FileSize]
        );
        assert_eq!(
            serde_json::to_value(result.limit_reasons[0]).unwrap(),
            serde_json::json!("fileSizeLimit")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn every_scan_budget_returns_a_bounded_partial_response() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let result_limited = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..SearchLimits::default()
        })
        .search(
            WorkspaceSearchRequest {
                project_id: "project-test".into(),
                query: "needle".into(),
                case_sensitive: false,
            },
            vec![root],
        )
        .unwrap();
        assert_eq!(result_limited.matches.len(), 1);
        assert!(
            result_limited
                .limit_reasons
                .contains(&super::types::SearchLimitReason::Results)
        );

        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let root_with = |workspace| SearchRoot {
            root_id: "root-test".into(),
            root_name: "fixture".into(),
            source: SearchRootSource::Available(workspace),
        };
        for (limits, expected) in [
            (
                SearchLimits {
                    max_files: 1,
                    ..SearchLimits::default()
                },
                super::types::SearchLimitReason::FileCount,
            ),
            (
                SearchLimits {
                    max_total_bytes: 1,
                    ..SearchLimits::default()
                },
                super::types::SearchLimitReason::Bytes,
            ),
            (
                SearchLimits {
                    max_entries: 1,
                    ..SearchLimits::default()
                },
                super::types::SearchLimitReason::Entries,
            ),
            (
                SearchLimits {
                    max_depth: 0,
                    ..SearchLimits::default()
                },
                super::types::SearchLimitReason::Depth,
            ),
            (
                SearchLimits {
                    max_duration: Duration::ZERO,
                    ..SearchLimits::default()
                },
                super::types::SearchLimitReason::Time,
            ),
        ] {
            let response = SearchEngine::new(limits)
                .search(
                    WorkspaceSearchRequest {
                        project_id: "project-test".into(),
                        query: "not-present".into(),
                        case_sensitive: false,
                    },
                    vec![root_with(workspace.clone())],
                )
                .unwrap();
            assert!(response.truncated, "{expected:?}");
            assert!(response.limit_reasons.contains(&expected), "{expected:?}");
            assert!(
                response.roots[0].state == super::types::SearchRootState::Partial,
                "{expected:?}"
            );
            assert!(response.matches.len() <= limits.max_results);
            assert!(response.roots[0].files_scanned <= limits.max_files);
            assert!(response.roots[0].bytes_scanned <= limits.max_total_bytes);
            assert!(response.roots[0].entries_visited <= limits.max_entries);
        }
        fs::remove_dir_all(path).unwrap();
    }
}
