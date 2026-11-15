use sage_core::error::{AppError, AppResult};
use serde::Deserialize;
use tauri::State;

use crate::{
    repository,
    repository_discovery::{self, MAX_REPOSITORIES_PER_PROJECT},
    state::AppState,
    workspace::Workspace,
};
use sage_domain::{
    editor::FileDiff,
    repository::{RepositoryRef, RootRepositoryStatus},
    resource::ResourceRef,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedRepositoryRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub r#ref: RepositoryRef,
    pub resource: Option<ResourceRef>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedRepositoryFileDiffResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub diff: FileDiff,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedRepositoryStatusResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub r#ref: RepositoryRef,
    pub status: RootRepositoryStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverProjectRepositoriesRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverProjectRepositoriesResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub refs: Vec<RepositoryRef>,
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResourceRepositoryRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub resource: ResourceRef,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResourceRepositoryResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub repository: Option<RepositoryRef>,
}

#[tauri::command]
pub async fn resolve_resource_repository(
    state: State<'_, AppState>,
    request: ResolveResourceRepositoryRequest,
) -> Result<ResolveResourceRepositoryResult, String> {
    resolve_inner(&state, request)
        .await
        .map_err(|error| error.to_string())
}

async fn resolve_inner(
    state: &AppState,
    request: ResolveResourceRepositoryRequest,
) -> AppResult<ResolveResourceRepositoryResult> {
    let _ = request.project_generation;
    let epoch = admit_project(
        state,
        &request.project_id,
        request.expected_transition_epoch,
    )
    .await?;
    let workspace = nested_workspace(
        state,
        &request.project_id,
        &RepositoryRef {
            root_id: request.resource.root_id.clone(),
            relative_path: String::new(),
        },
    )?;
    let repository = workspace.resource_repository(&request.resource)?;
    Ok(ResolveResourceRepositoryResult {
        project_id: request.project_id,
        transition_epoch: epoch,
        repository,
    })
}

#[tauri::command]
pub async fn discover_project_repositories(
    state: State<'_, AppState>,
    request: DiscoverProjectRepositoriesRequest,
) -> Result<DiscoverProjectRepositoriesResult, String> {
    discover_inner(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_repository_ref_status(
    state: State<'_, AppState>,
    request: NestedRepositoryRequest,
) -> Result<NestedRepositoryStatusResult, String> {
    status_inner(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_repository_ref_file_diff(
    state: State<'_, AppState>,
    request: NestedRepositoryRequest,
) -> Result<NestedRepositoryFileDiffResult, String> {
    diff_inner(&state, request)
        .await
        .map_err(|error| error.to_string())
}

async fn discover_inner(
    state: &AppState,
    request: DiscoverProjectRepositoriesRequest,
) -> AppResult<DiscoverProjectRepositoriesResult> {
    let _ = request.project_generation;
    let epoch = admit_project(
        state,
        &request.project_id,
        request.expected_transition_epoch,
    )
    .await?;
    let roots = state.database.root_records(&request.project_id)?;
    let mut refs = Vec::new();
    let mut truncated = false;
    for root in roots {
        let remaining = MAX_REPOSITORIES_PER_PROJECT.saturating_sub(refs.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        let discovery =
            repository_discovery::discover_root(&root.id, &root.absolute_path, remaining);
        truncated |= discovery.truncated;
        refs.extend(discovery.refs);
    }
    if refs.len() > MAX_REPOSITORIES_PER_PROJECT {
        refs.truncate(MAX_REPOSITORIES_PER_PROJECT);
        truncated = true;
    }
    Ok(DiscoverProjectRepositoriesResult {
        project_id: request.project_id,
        transition_epoch: epoch,
        refs,
        truncated,
    })
}

async fn status_inner(
    state: &AppState,
    request: NestedRepositoryRequest,
) -> AppResult<NestedRepositoryStatusResult> {
    let _ = request.project_generation;
    let epoch = admit_project(
        state,
        &request.project_id,
        request.expected_transition_epoch,
    )
    .await?;
    let workspace = nested_workspace(state, &request.project_id, &request.r#ref)?;
    let mut status = repository::status(&workspace).await?;
    prefix_status_paths(&request.r#ref, &mut status);
    Ok(NestedRepositoryStatusResult {
        project_id: request.project_id,
        transition_epoch: epoch,
        r#ref: request.r#ref,
        status,
    })
}

async fn diff_inner(
    state: &AppState,
    request: NestedRepositoryRequest,
) -> AppResult<NestedRepositoryFileDiffResult> {
    let _ = request.project_generation;
    let epoch = admit_project(
        state,
        &request.project_id,
        request.expected_transition_epoch,
    )
    .await?;
    if request.r#ref.relative_path.is_empty() {
        return Err(AppError::InvalidRequest(
            "nested repository Diff requires a nested RepositoryRef; use the root Diff command"
                .into(),
        ));
    }
    let resource = request.resource.ok_or_else(|| {
        AppError::InvalidRequest("nested repository Diff requires a resource".into())
    })?;
    let git_relative = git_relative_path(&request.r#ref, &resource)?;
    let workspace = nested_workspace(state, &request.project_id, &request.r#ref)?;
    let remapped = ResourceRef {
        root_id: request.r#ref.root_id.clone(),
        relative_path: git_relative,
    };
    let mut diff = repository::file_diff(&workspace, &remapped).await?;
    diff.resource = resource;
    Ok(NestedRepositoryFileDiffResult {
        project_id: request.project_id,
        transition_epoch: epoch,
        diff,
    })
}

async fn admit_project(
    state: &AppState,
    project_id: &str,
    expected_transition_epoch: u64,
) -> AppResult<u64> {
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested repository command".into(),
        ));
    }
    let active = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active != project_id {
        return Err(AppError::InvalidRequest(
            "repository command does not belong to the active Project".into(),
        ));
    }
    Ok(*transition_epoch)
}

fn nested_workspace(
    state: &AppState,
    project_id: &str,
    repository: &RepositoryRef,
) -> AppResult<Workspace> {
    let roots = state.database.root_records(project_id)?;
    let root = roots
        .iter()
        .find(|root| root.id == repository.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest("repository root does not belong to the active Project".into())
        })?;
    let path = contained_relative(&root.absolute_path, &repository.relative_path)?;
    Workspace::open(project_id, &root.id, &path)
}

fn contained_relative(root: &std::path::Path, relative: &str) -> AppResult<std::path::PathBuf> {
    let mut path = root.to_path_buf();
    if !relative.is_empty() {
        for part in relative.split(['/', '\\']).filter(|part| !part.is_empty()) {
            if part == "." || part == ".." {
                return Err(AppError::InvalidRequest(
                    "repository relative path must stay inside its Project root".into(),
                ));
            }
            path.push(part);
        }
    }
    let root = root.canonicalize()?;
    let path = path
        .canonicalize()
        .map_err(|_| AppError::InvalidRequest("selected repository path does not exist".into()))?;
    if !path.starts_with(&root) {
        return Err(AppError::InvalidRequest(
            "repository relative path must stay inside its Project root".into(),
        ));
    }
    Ok(path)
}

fn git_relative_path(repository: &RepositoryRef, resource: &ResourceRef) -> AppResult<String> {
    if resource.root_id != repository.root_id {
        return Err(AppError::InvalidRequest(
            "repository root does not match the Diff resource root".into(),
        ));
    }
    if repository.relative_path.is_empty() {
        return Ok(resource.relative_path.clone());
    }
    if resource.relative_path == repository.relative_path {
        return Err(AppError::InvalidRequest(
            "nested Diff requires a file inside the selected repository".into(),
        ));
    }
    let prefix = format!("{}/", repository.relative_path);
    resource
        .relative_path
        .strip_prefix(&prefix)
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::InvalidRequest("Diff resource is outside the selected repository".into())
        })
}

fn prefix_status_paths(repository: &RepositoryRef, status: &mut RootRepositoryStatus) {
    if repository.relative_path.is_empty() {
        return;
    }
    for change in &mut status.changes {
        change.resource.relative_path =
            prefix_relative(&repository.relative_path, &change.resource.relative_path);
        if let Some(original) = &mut change.original_resource {
            original.relative_path =
                prefix_relative(&repository.relative_path, &original.relative_path);
        }
    }
}

fn prefix_relative(repository: &str, inner: &str) -> String {
    if inner.is_empty() {
        repository.to_string()
    } else {
        format!("{repository}/{inner}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    use uuid::Uuid;

    use crate::{persistence::Database, state::AppState};

    fn git(root: &Path, args: &[&str]) {
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    fn fixture() -> (AppState, String, String, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("sage-nested-ref-{}", Uuid::new_v4()));
        let root = base.join("repo");
        let nested = root.join("packages").join("tool");
        fs::create_dir_all(&nested).unwrap();
        git(&root, &["init", "-q"]);
        git(&nested, &["init", "-q"]);
        git(&nested, &["config", "user.email", "sage@example.invalid"]);
        git(&nested, &["config", "user.name", "Sage Test"]);
        fs::write(nested.join("app.ts"), "export const n = 1;\n").unwrap();
        git(&nested, &["add", "--", "app.ts"]);
        git(&nested, &["commit", "-q", "-m", "fixture"]);
        fs::write(nested.join("app.ts"), "export const n = 2;\n").unwrap();
        let database = Database::open(&base.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let root_id = snapshot.project.unwrap().roots[0].id.clone();
        (AppState::new(database), project.id, root_id, base)
    }

    #[test]
    fn discover_returns_root_and_nested_repository_refs() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project_id, root_id, base) = fixture();
            let result = discover_inner(
                &state,
                DiscoverProjectRepositoriesRequest {
                    project_id: project_id.clone(),
                    project_generation: 0,
                    expected_transition_epoch: 0,
                },
            )
            .await
            .unwrap();
            assert_eq!(result.project_id, project_id);
            assert!(
                result
                    .refs
                    .iter()
                    .any(|r| r.root_id == root_id && r.relative_path.is_empty()),
                "root repository must be discovered"
            );
            assert!(
                result
                    .refs
                    .iter()
                    .any(|r| r.root_id == root_id && r.relative_path == "packages/tool"),
                "nested repository must be discovered: {:?}",
                result.refs
            );
            drop(state);
            fs::remove_dir_all(base).ok();
        });
    }

    #[test]
    fn nested_status_and_diff_use_the_nested_git_top_level() {
        for root_repository in [true, false] {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let (state, project_id, root_id, base) = fixture();
                if !root_repository {
                    fs::rename(base.join("repo/.git"), base.join("root-git-marker")).unwrap();
                }
                let resource = ResourceRef {
                    root_id: root_id.clone(),
                    relative_path: "packages/tool/app.ts".into(),
                };
                let resolved = resolve_inner(
                    &state,
                    ResolveResourceRepositoryRequest {
                        project_id: project_id.clone(),
                        project_generation: 0,
                        expected_transition_epoch: 0,
                        resource: resource.clone(),
                    },
                )
                .await
                .unwrap();
                let repository = resolved.repository.unwrap();
                assert_eq!(repository.relative_path, "packages/tool");
                let status = status_inner(
                    &state,
                    NestedRepositoryRequest {
                        project_id: project_id.clone(),
                        project_generation: 0,
                        expected_transition_epoch: 0,
                        r#ref: repository.clone(),
                        resource: None,
                    },
                )
                .await
                .unwrap();
                assert_eq!(status.status.state, "repository");
                assert_eq!(status.status.changes[0].resource, resource);

                let diff = diff_inner(
                    &state,
                    NestedRepositoryRequest {
                        project_id,
                        project_generation: 0,
                        expected_transition_epoch: 0,
                        r#ref: repository,
                        resource: Some(resource.clone()),
                    },
                )
                .await
                .unwrap();
                assert_eq!(diff.diff.resource, resource);
                assert!(!diff.diff.hunks.is_empty());
                drop(state);
                fs::remove_dir_all(base).ok();
            });
        }
    }

    #[test]
    fn nested_status_rejects_parent_directory_escape() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project_id, root_id, base) = fixture();
            let error = status_inner(
                &state,
                NestedRepositoryRequest {
                    project_id,
                    project_generation: 0,
                    expected_transition_epoch: 0,
                    r#ref: RepositoryRef {
                        root_id,
                        relative_path: "..".into(),
                    },
                    resource: None,
                },
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("inside its Project root"));
            drop(state);
            fs::remove_dir_all(base).ok();
        });
    }
}
