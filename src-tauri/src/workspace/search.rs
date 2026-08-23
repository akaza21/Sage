pub(crate) mod command;
mod contained_file;
mod engine;
mod file_scan;
mod helper_process;
mod helper_protocol;
pub(crate) mod language_fixture;
mod types;

pub use helper_protocol::run_stdio_helper_if_requested;

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use uuid::Uuid;

    use super::{
        engine::SearchEngine,
        types::{SearchLimits, SearchRoot, SearchRootSource, WorkspaceSearchRequest},
    };
    use crate::workspace::Workspace;

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

    pub(super) fn request(query: &str) -> WorkspaceSearchRequest {
        WorkspaceSearchRequest {
            request_id: "request-test".into(),
            project_id: "project-test".into(),
            expected_transition_epoch: 0,
            query: query.into(),
            case_sensitive: false,
        }
    }

    #[test]
    fn finds_a_literal_with_a_canonical_resource_identity() {
        let (path, root) = fixture_root();
        let result = SearchEngine::new(SearchLimits::default())
            .search(request("hello sage"), vec![root])
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
    fn oversized_files_are_skipped_without_crossing_the_per_file_bound() {
        let (path, root) = fixture_root();
        fs::write(path.join("large.txt"), "needle is beyond the bound").unwrap();
        let limits = SearchLimits {
            max_file_bytes: 8,
            ..SearchLimits::default()
        };
        let result = SearchEngine::new(limits)
            .search(request("needle"), vec![root])
            .unwrap();
        assert!(result.matches.is_empty());
        assert_eq!(result.roots[0].files_scanned, 0);
        assert_eq!(result.roots[0].skipped_files, 2);
        assert_eq!(
            result.limit_reasons,
            vec![super::types::SearchLimitReason::FileSize]
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
        .search(request("needle"), vec![root])
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
                .search(request("not-present"), vec![root_with(workspace.clone())])
                .unwrap();
            assert!(response.truncated, "{expected:?}");
            assert!(response.limit_reasons.contains(&expected), "{expected:?}");
            assert_eq!(
                response.roots[0].state,
                super::types::SearchRootState::Partial
            );
        }
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn directory_links_never_expose_matches_outside_the_root() {
        let (root_path, root) = fixture_root();
        let outside = root_path.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "outside-only-needle\n").unwrap();
        let link = root_path.join("linked-outside");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
            fs::remove_dir_all(root_path).unwrap();
            fs::remove_dir_all(outside).unwrap();
            return;
        }

        let result = SearchEngine::new(SearchLimits::default())
            .search(request("outside-only-needle"), vec![root])
            .unwrap();
        assert!(result.matches.is_empty());
        fs::remove_dir_all(root_path).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
