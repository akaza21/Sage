pub(crate) mod command;
mod contained_file;
mod engine;
mod engine_continue;
mod engine_walk;
mod file_scan;
mod helper_process;
mod helper_protocol;
mod literal_match;
mod path_filter;
mod types;

pub use helper_protocol::run_stdio_helper_if_requested;

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use uuid::Uuid;

    use super::{
        engine::SearchEngine,
        types::{
            ContinuationCursor, SearchLimits, SearchRoot, SearchRootSource, WorkspaceSearchRequest,
            encode_continuation_token,
        },
    };
    use crate::workspace::Workspace;

    fn test_limits() -> SearchLimits {
        SearchLimits {
            max_duration: crate::test_wait::BUDGET,
            ..SearchLimits::default()
        }
    }

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
            index_generation: 0,
            continuation_token: None,
            query: query.into(),
            case_sensitive: false,
            whole_word: false,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            use_ignore_files: true,
            files_exclude: None,
        }
    }

    #[test]
    fn leaves_out_what_files_exclude_names() {
        let (path, _) = fixture_root();
        fs::create_dir_all(path.join("web").join("dist")).unwrap();
        for file in [".DS_Store", "web/dist/app.js", "web/app.tmp", "web/app.ts"] {
            fs::write(path.join(file), "needle\n").unwrap();
        }
        let found = |files_exclude: Option<Vec<String>>| {
            let root = SearchRoot {
                root_id: "root-test".into(),
                root_name: "fixture".into(),
                source: SearchRootSource::Available(
                    Workspace::open("project-test", "root-test", &path).unwrap(),
                ),
            };
            let mut paths = SearchEngine::new(test_limits())
                .search(
                    WorkspaceSearchRequest {
                        files_exclude,
                        ..request("needle")
                    },
                    vec![root],
                )
                .unwrap()
                .matches
                .into_iter()
                .map(|found| found.resource.relative_path)
                .collect::<Vec<_>>();
            paths.sort();
            paths
        };
        // The registry default leaves out `.DS_Store`.
        assert_eq!(
            found(None),
            ["web/app.tmp", "web/app.ts", "web/dist/app.js"]
        );
        assert_eq!(
            found(Some(vec!["**/dist".into(), "**/*.tmp".into()])),
            [".DS_Store", "web/app.ts"]
        );
        assert_eq!(found(Some(Vec::new())).len(), 4);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn refuses_invalid_files_exclude_patterns() {
        for patterns in [
            vec!["/absolute".to_string()],
            vec!["dist/".into()],
            vec!["**/x".into(); crate::settings_store::MAX_GLOBS + 1],
        ] {
            let (path, root) = fixture_root();
            assert!(
                SearchEngine::new(test_limits())
                    .search(
                        WorkspaceSearchRequest {
                            files_exclude: Some(patterns),
                            ..request("needle")
                        },
                        vec![root],
                    )
                    .is_err()
            );
            fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn skips_what_ignore_files_ignore_unless_asked_not_to() {
        let (path, root) = fixture_root();
        fs::write(path.join(".gitignore"), "*.log\nbuild/\n!keep.log\n").unwrap();
        fs::create_dir_all(path.join("build")).unwrap();
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("src").join(".ignore"), "scratch.txt\n").unwrap();
        for file in [
            "kept.txt",
            "debug.log",
            "keep.log",
            "build/out.txt",
            "src/scratch.txt",
            "src/lib.txt",
        ] {
            fs::write(path.join(file), "needle\n").unwrap();
        }
        let found = |use_ignore_files: bool, root| {
            let mut paths = SearchEngine::new(test_limits())
                .search(
                    WorkspaceSearchRequest {
                        use_ignore_files,
                        ..request("needle")
                    },
                    vec![root],
                )
                .unwrap()
                .matches
                .into_iter()
                .map(|found| found.resource.relative_path)
                .collect::<Vec<_>>();
            paths.sort();
            paths
        };
        assert_eq!(found(true, root), ["keep.log", "kept.txt", "src/lib.txt"]);
        let root = SearchRoot {
            root_id: "root-test".into(),
            root_name: "fixture".into(),
            source: SearchRootSource::Available(
                Workspace::open("project-test", "root-test", &path).unwrap(),
            ),
        };
        assert_eq!(
            found(false, root),
            [
                "build/out.txt",
                "debug.log",
                "keep.log",
                "kept.txt",
                "src/lib.txt",
                "src/scratch.txt"
            ]
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn skips_git_and_node_modules_trees() {
        let (path, root) = fixture_root();
        fs::create_dir_all(path.join(".git")).unwrap();
        fs::create_dir_all(path.join("node_modules").join("pkg")).unwrap();
        fs::write(path.join(".git").join("config"), "needle in git\n").unwrap();
        fs::write(
            path.join("node_modules").join("pkg").join("index.js"),
            "needle in modules\n",
        )
        .unwrap();
        fs::write(path.join("src").join("main.rs"), "const needle = true;\n").unwrap();

        let result = SearchEngine::new(test_limits())
            .search(request("needle"), vec![root])
            .unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].resource.relative_path, "src/main.rs");
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn finds_a_literal_with_a_canonical_resource_identity() {
        let (path, root) = fixture_root();
        let result = SearchEngine::new(test_limits())
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
    fn whole_word_search_excludes_identifier_substrings_and_preserves_case_mode() {
        let (path, root) = fixture_root();
        fs::write(
            path.join("whole-word.txt"),
            "needle needles needle_value NEEDLE (needle)\n",
        )
        .unwrap();
        let result = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    whole_word: true,
                    ..request("needle")
                },
                vec![root],
            )
            .unwrap();

        let matches = result
            .matches
            .iter()
            .filter(|match_value| match_value.resource.relative_path == "whole-word.txt")
            .map(|match_value| match_value.column_number)
            .collect::<Vec<_>>();
        assert_eq!(matches, vec![1, 29, 37]);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn path_filters_include_matching_files_then_subtract_exclusions() {
        let (path, root) = fixture_root();
        fs::write(path.join("src").join("kept.rs"), "needle\n").unwrap();
        fs::write(path.join("src").join("ignored.rs"), "needle\n").unwrap();
        fs::write(path.join("notes.txt"), "needle\n").unwrap();

        let result = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    include_patterns: vec!["src/**/*.rs".into()],
                    exclude_patterns: vec!["**/ignored.rs".into()],
                    ..request("needle")
                },
                vec![root],
            )
            .unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].resource.relative_path, "src/kept.rs");
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn invalid_path_filters_fail_closed_before_walking() {
        let (path, _root) = fixture_root();
        for include_patterns in [
            vec!["../outside".into()],
            vec!["/absolute".into()],
            vec!["C:/drive".into()],
            vec!["line\nbreak".into()],
        ] {
            let error = SearchEngine::new(test_limits())
                .search(
                    WorkspaceSearchRequest {
                        include_patterns,
                        ..request("needle")
                    },
                    vec![SearchRoot {
                        root_id: "root-test".into(),
                        root_name: "fixture".into(),
                        source: SearchRootSource::Available(
                            Workspace::open("project-test", "root-test", &path).unwrap(),
                        ),
                    }],
                )
                .unwrap_err();
            assert!(error.to_string().contains("workspace search path filter"));
        }
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn oversized_files_are_skipped_without_crossing_the_per_file_bound() {
        let (path, root) = fixture_root();
        fs::write(path.join("large.txt"), "needle is beyond the bound").unwrap();
        let limits = SearchLimits {
            max_file_bytes: 8,
            ..test_limits()
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
    fn binary_files_are_skipped_without_making_the_search_partial() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle needle\n").unwrap();
        fs::write(path.join("image.bin"), b"needle\0\x89PNG").unwrap();
        let complete = SearchEngine::new(test_limits())
            .search(request("needle"), vec![root])
            .unwrap();
        assert_eq!(complete.matches.len(), 3);
        assert_eq!(complete.roots[0].skipped_files, 1);
        assert_eq!(
            complete.roots[0].state,
            super::types::SearchRootState::Complete
        );
        assert!(!complete.truncated);
        assert!(complete.limit_reasons.is_empty());

        // A continued page that walks past the same files stays complete too.
        let limits = SearchLimits {
            max_results: 2,
            ..test_limits()
        };
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let root_with = |workspace| SearchRoot {
            root_id: "root-test".into(),
            root_name: "fixture".into(),
            source: SearchRootSource::Available(workspace),
        };
        let first = SearchEngine::new(limits)
            .search(request("needle"), vec![root_with(workspace.clone())])
            .unwrap();
        let second = SearchEngine::new(limits)
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token.clone(),
                    ..request("needle")
                },
                vec![root_with(workspace)],
            )
            .unwrap();
        assert_eq!(second.matches.len(), 1);
        assert_eq!(second.matches[0].column_number, 15);
        assert_eq!(second.roots[0].skipped_files, 1);
        assert_eq!(
            second.roots[0].state,
            super::types::SearchRootState::Complete
        );
        assert!(second.continuation_token.is_none());

        // Text that is not UTF-8 was never searched, so the root says so.
        fs::write(path.join("latin1.txt"), b"needle caf\xe9\n").unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let partial = SearchEngine::new(test_limits())
            .search(request("needle"), vec![root_with(workspace)])
            .unwrap();
        assert_eq!(partial.roots[0].skipped_files, 2);
        assert_eq!(
            partial.roots[0].state,
            super::types::SearchRootState::Partial
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn every_scan_budget_returns_a_bounded_partial_response() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let result_limited = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
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
                    ..test_limits()
                },
                super::types::SearchLimitReason::FileCount,
            ),
            (
                SearchLimits {
                    max_total_bytes: 1,
                    ..test_limits()
                },
                super::types::SearchLimitReason::Bytes,
            ),
            (
                SearchLimits {
                    max_entries: 1,
                    ..test_limits()
                },
                super::types::SearchLimitReason::Entries,
            ),
            (
                SearchLimits {
                    max_depth: 0,
                    ..test_limits()
                },
                super::types::SearchLimitReason::Depth,
            ),
            (
                SearchLimits {
                    max_duration: Duration::ZERO,
                    ..test_limits()
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
    fn result_limited_first_page_returns_a_token_and_the_second_page_starts_after_the_last_match() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let limits = SearchLimits {
            max_results: 1,
            ..test_limits()
        };
        let first = SearchEngine::new(limits)
            .search(request("needle"), vec![root])
            .unwrap();
        let token = first.continuation_token.clone().unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let second = SearchEngine::new(limits)
            .search(
                WorkspaceSearchRequest {
                    continuation_token: Some(token),
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap();

        assert_eq!(first.matches[0].column_number, 1);
        assert_eq!(second.matches[0].column_number, 8);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn continuation_with_a_different_query_or_index_generation_is_stale() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let first = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
        })
        .search(request("needle"), vec![root])
        .unwrap();
        for mut continued in [request("other"), request("needle")] {
            continued.continuation_token = first.continuation_token.clone();
            if continued.query == "needle" {
                continued.index_generation = 1;
            }
            let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
            let error = SearchEngine::new(test_limits())
                .search(
                    continued,
                    vec![SearchRoot {
                        root_id: "root-test".into(),
                        root_name: "fixture".into(),
                        source: SearchRootSource::Available(workspace),
                    }],
                )
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("workspace search continuation is stale")
            );
        }
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn continuation_with_a_different_whole_word_mode_is_stale() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let first = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
        })
        .search(
            WorkspaceSearchRequest {
                whole_word: true,
                ..request("needle")
            },
            vec![root],
        )
        .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token,
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is stale")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn continuation_with_a_different_ignore_file_choice_is_stale() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let first = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
        })
        .search(request("needle"), vec![root])
        .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token,
                    use_ignore_files: false,
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is stale")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn continuation_with_different_files_exclude_patterns_is_stale() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let first = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
        })
        .search(request("needle"), vec![root])
        .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token,
                    files_exclude: Some(vec!["**/*.md".into()]),
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is stale")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn continuation_with_different_path_filters_is_stale() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let first = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
        })
        .search(
            WorkspaceSearchRequest {
                include_patterns: vec!["**/*.txt".into()],
                ..request("needle")
            },
            vec![root],
        )
        .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token,
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is stale")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn continuation_whose_resume_file_was_removed_is_stale() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let first = SearchEngine::new(SearchLimits {
            max_results: 1,
            ..test_limits()
        })
        .search(request("needle"), vec![root])
        .unwrap();
        fs::remove_file(path.join("another.txt")).unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token,
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is stale")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn malformed_continuation_token_is_invalid() {
        let (path, root) = fixture_root();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: Some("not-json".into()),
                    ..request("needle")
                },
                vec![root],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is invalid")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn oversize_continuation_token_is_invalid() {
        let (path, root) = fixture_root();
        let error = SearchEngine::new(test_limits())
            .search(
                WorkspaceSearchRequest {
                    continuation_token: Some("x".repeat(1025)),
                    ..request("needle")
                },
                vec![root],
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is invalid")
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn oversized_encoded_token_is_omitted() {
        let token = encode_continuation_token(
            &request("needle"),
            "needle",
            &ContinuationCursor {
                root_id: "root-test".into(),
                relative_path: "a".repeat(2000),
                line_number: 1,
                column_number: 1,
            },
        );
        assert!(token.is_none());
    }

    #[test]
    fn post_cursor_siblings_charge_the_page_entry_budget() {
        let (path, root) = fixture_root();
        fs::write(path.join("another.txt"), "needle needle\n").unwrap();
        let limits = SearchLimits {
            max_results: 1,
            ..test_limits()
        };
        let first = SearchEngine::new(limits)
            .search(request("needle"), vec![root])
            .unwrap();
        let workspace = Workspace::open("project-test", "root-test", &path).unwrap();
        let second = SearchEngine::new(limits)
            .search(
                WorkspaceSearchRequest {
                    continuation_token: first.continuation_token,
                    ..request("needle")
                },
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(workspace),
                }],
            )
            .unwrap();
        assert!(second.roots[0].entries_visited > 0);
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

        let result = SearchEngine::new(test_limits())
            .search(request("outside-only-needle"), vec![root])
            .unwrap();
        assert!(result.matches.is_empty());
        fs::remove_dir_all(root_path).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn production_file_walk_admits_200k_and_fail_closes_one_over() {
        use super::types::{MAX_SEARCH_ENTRIES, MAX_SEARCH_FILES, MAX_SEARCH_RESULTS};
        use crate::workspace::file_walk::{
            WalkBufferAdmit, admit_queued_directory, admit_walk_dirent, walk_over_cap,
        };
        use std::ffi::OsStr;

        assert_eq!(MAX_SEARCH_FILES, 200_000);
        assert_eq!(MAX_SEARCH_ENTRIES, 200_000);
        assert_eq!(MAX_SEARCH_RESULTS, 500);
        assert_eq!(crate::workspace::MAX_DIRECTORY_ENTRIES, 500);
        assert!(!walk_over_cap(200_000, MAX_SEARCH_FILES));
        assert!(walk_over_cap(200_001, MAX_SEARCH_FILES));
        assert!(!walk_over_cap(200_000, MAX_SEARCH_ENTRIES));
        assert!(walk_over_cap(200_001, MAX_SEARCH_ENTRIES));
        assert_eq!(
            admit_walk_dirent(OsStr::new(".git"), 0, 8),
            WalkBufferAdmit::Skip
        );
        assert_eq!(
            admit_walk_dirent(OsStr::new("node_modules"), 0, 8),
            WalkBufferAdmit::Skip
        );
        assert_eq!(
            admit_walk_dirent(OsStr::new("src"), 8, 8),
            WalkBufferAdmit::Cap
        );
        assert!(admit_queued_directory(4_095, 4_096));
        assert!(!admit_queued_directory(4_096, 4_096));
    }
}
