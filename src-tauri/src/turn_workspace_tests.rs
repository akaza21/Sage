use super::*;

fn temp_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sage-turn-workspace-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn porcelain_records_become_a_bounded_fingerprint() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    std::fs::write(root.join("a.txt"), b"one").unwrap();
    let anchor = directory::Directory::root(&root).unwrap();
    let mark = parse_mark(&anchor, b" M a.txt\0?? new.txt\0bad\0", false).unwrap();
    assert_eq!(mark.entries.len(), 2);
    assert_eq!(mark.entries["a.txt"].len, Some(3));
    assert_eq!(mark.entries["new.txt"].len, None);
    let many: Vec<u8> = (0..MAX_TURN_WORKSPACE_CHANGES + 5)
        .flat_map(|index| format!("?? f{index}\0").into_bytes())
        .collect();
    let bounded = parse_mark(&anchor, &many, false).unwrap();
    assert_eq!(bounded.entries.len(), MAX_TURN_WORKSPACE_CHANGES);
    assert!(bounded.truncated);
    drop(anchor);
    std::fs::remove_dir_all(root).unwrap();
}

fn mark(entries: &[(&str, &str, Option<u64>, u64)]) -> WorkspaceMark {
    WorkspaceMark {
        root_identity: Some([1; 24]),
        entries: entries
            .iter()
            .map(|(path, status, len, modified)| {
                (
                    (*path).to_owned(),
                    Fingerprint {
                        status: (*status).to_owned(),
                        len: *len,
                        modified_nanos: Some(*modified),
                    },
                )
            })
            .collect(),
        truncated: false,
        head: None,
        committed: BTreeMap::new(),
        terminal_presence: BTreeMap::new(),
    }
}

fn git_in(root: &std::path::Path, args: &[&str]) {
    assert!(
        crate::test_git::command()
            .args(["-c", "user.name=Test", "-c", "user.email=fixture"])
            .args(args)
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn tracked_dependency_changes_do_not_consume_dirty_or_committed_source_capacity() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    git_in(&root, &["init", "-q"]);
    let dependencies = root.join("packages/app/node_modules/pkg");
    std::fs::create_dir_all(&dependencies).unwrap();
    for index in 0..301 {
        std::fs::write(dependencies.join(format!("{index}.js")), "before").unwrap();
    }
    std::fs::write(root.join("source.rs"), "before").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-qm", "baseline"]);
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        for index in 0..301 {
            std::fs::write(dependencies.join(format!("{index}.js")), "after").unwrap();
        }
        std::fs::write(root.join("source.rs"), "source edit").unwrap();
        let dirty = capture_after(&root, &before).await.unwrap();
        let changes = diff_marks(&before, &dirty);
        assert!(changes.tracked && !changes.truncated);
        assert_eq!(
            changes
                .changes
                .iter()
                .map(|change| change.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["source.rs"]
        );
        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-qm", "turn edits"]);
        let committed = capture_after(&root, &before).await.unwrap();
        assert_eq!(diff_marks(&before, &committed), changes);
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_a_baseline_untracked_file_is_a_deleted_turn_change() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    git_in(&root, &["init", "-q"]);
    std::fs::create_dir(root.join("scratch")).unwrap();
    std::fs::write(root.join("scratch/untracked.txt"), "before the turn").unwrap();
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        assert_eq!(before.entries["scratch/untracked.txt"].status, "??");
        std::fs::remove_file(root.join("scratch/untracked.txt")).unwrap();
        let after = capture_after(&root, &before).await.unwrap();
        let changes = diff_marks(&before, &after);
        assert!(changes.tracked && !changes.truncated);
        assert_eq!(
            changes.changes,
            [TurnWorkspaceChange {
                relative_path: "scratch/untracked.txt".into(),
                operation: TurnWorkspaceOperation::Deleted,
            }]
        );
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_an_untracked_parent_directory_is_a_deleted_turn_change() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    git_in(&root, &["init", "-q"]);
    std::fs::create_dir(root.join("scratch")).unwrap();
    std::fs::write(root.join("scratch/untracked.txt"), "before the turn").unwrap();
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        std::fs::remove_dir_all(root.join("scratch")).unwrap();
        let after = capture_after(&root, &before).await.unwrap();
        assert_eq!(
            diff_marks(&before, &after).changes,
            [TurnWorkspaceChange {
                relative_path: "scratch/untracked.txt".into(),
                operation: TurnWorkspaceOperation::Deleted,
            }]
        );
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn baseline_files_becoming_clean_or_committed_are_not_deleted() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    git_in(&root, &["init", "-q"]);
    std::fs::write(root.join("reverted.txt"), "original").unwrap();
    std::fs::write(root.join("committed.txt"), "original").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-q", "-m", "init"]);
    std::fs::write(root.join("reverted.txt"), "dirty").unwrap();
    std::fs::write(root.join("committed.txt"), "dirty").unwrap();
    std::fs::write(root.join("added.txt"), "untracked").unwrap();
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        git_in(&root, &["restore", "reverted.txt"]);
        git_in(&root, &["add", "committed.txt", "added.txt"]);
        git_in(&root, &["commit", "-q", "-m", "settle dirty files"]);
        let after = capture_after(&root, &before).await.unwrap();
        assert!(after.entries.is_empty());
        let changes = diff_marks(&before, &after);
        assert!(changes.tracked && !changes.truncated);
        assert_eq!(
            changes.changes,
            [
                TurnWorkspaceChange {
                    relative_path: "added.txt".into(),
                    operation: TurnWorkspaceOperation::Created,
                },
                TurnWorkspaceChange {
                    relative_path: "committed.txt".into(),
                    operation: TurnWorkspaceOperation::Updated,
                },
                TurnWorkspaceChange {
                    relative_path: "reverted.txt".into(),
                    operation: TurnWorkspaceOperation::Updated,
                },
            ]
        );
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn baseline_only_paths_with_substituted_symlinks_fail_closed() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    for replace_parent in [false, true] {
        let root = temp_root();
        let outside = temp_root();
        git_in(&root, &["init", "-q"]);
        std::fs::create_dir(root.join("scratch")).unwrap();
        std::fs::write(root.join("scratch/file.txt"), "baseline").unwrap();
        std::fs::write(outside.join("file.txt"), "external").unwrap();
        let root = root.canonicalize().unwrap();
        tauri::async_runtime::block_on(async {
            let before = capture(&root).await.unwrap();
            if replace_parent {
                std::fs::remove_dir_all(root.join("scratch")).unwrap();
                std::os::unix::fs::symlink(&outside, root.join("scratch")).unwrap();
            } else {
                std::fs::remove_file(root.join("scratch/file.txt")).unwrap();
                std::os::unix::fs::symlink(outside.join("file.txt"), root.join("scratch/file.txt"))
                    .unwrap();
            }
            assert!(capture_after(&root, &before).await.is_none());
        });
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn committing_a_substituted_symlink_does_not_bypass_baseline_validation() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    let outside = temp_root();
    git_in(&root, &["init", "-q"]);
    std::fs::write(root.join("file.txt"), "baseline").unwrap();
    std::fs::write(outside.join("file.txt"), "external").unwrap();
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        std::fs::remove_file(root.join("file.txt")).unwrap();
        std::os::unix::fs::symlink(outside.join("file.txt"), root.join("file.txt")).unwrap();
        git_in(&root, &["add", "file.txt"]);
        git_in(&root, &["commit", "-qm", "commit substituted symlink"]);
        assert!(capture_after(&root, &before).await.is_none());
    });
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn a_shell_edit_committed_before_the_turn_ends_is_a_turn_change() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    git_in(&root, &["init", "-q"]);
    std::fs::write(root.join("README.md"), b"hello\n").unwrap();
    std::fs::write(root.join("gone.txt"), b"bye\n").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-q", "-m", "init"]);
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        assert!(before.entries.is_empty());
        std::fs::write(root.join("README.md"), b"hello\nAudited.\n").unwrap();
        std::fs::write(root.join("added.txt"), b"new\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        git_in(&root, &["add", "-A"]);
        git_in(&root, &["commit", "-q", "-m", "shell commit"]);
        let after = capture_after(&root, &before).await.unwrap();
        assert!(after.entries.is_empty(), "both status captures are clean");
        let changes = diff_marks(&before, &after);
        assert!(changes.tracked && !changes.truncated);
        assert_eq!(
            changes.changes,
            [
                TurnWorkspaceChange {
                    relative_path: "README.md".into(),
                    operation: TurnWorkspaceOperation::Updated,
                },
                TurnWorkspaceChange {
                    relative_path: "added.txt".into(),
                    operation: TurnWorkspaceOperation::Created,
                },
                TurnWorkspaceChange {
                    relative_path: "gone.txt".into(),
                    operation: TurnWorkspaceOperation::Deleted,
                },
            ]
        );
    });
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_first_commit_in_an_unborn_repository_is_a_turn_change() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    git_in(&root, &["init", "-q"]);
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.unwrap();
        assert_eq!(before.head, Some(history::HeadMark::Unborn));
        std::fs::write(root.join("first.txt"), b"one\n").unwrap();
        git_in(&root, &["add", "first.txt"]);
        git_in(&root, &["commit", "-q", "-m", "first"]);
        let after = capture_after(&root, &before).await.unwrap();
        let changes = diff_marks(&before, &after);
        assert_eq!(
            changes.changes,
            [TurnWorkspaceChange {
                relative_path: "first.txt".into(),
                operation: TurnWorkspaceOperation::Created,
            }]
        );
    });
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn committed_paths_are_bounded_and_deduplicated_with_the_dirty_set() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let many: Vec<u8> = (0..MAX_TURN_WORKSPACE_CHANGES + 5)
        .flat_map(|index| format!("M\0f{index}\0").into_bytes())
        .collect();
    let (paths, truncated) = history::parse_name_status(&many);
    assert_eq!(paths.len(), MAX_TURN_WORKSPACE_CHANGES);
    assert!(truncated);
    let before = mark(&[("dirty.rs", " M", Some(1), 1)]);
    let mut after = mark(&[]);
    after.committed = [
        ("dirty.rs".to_owned(), TurnWorkspaceOperation::Deleted),
        ("clean.rs".to_owned(), TurnWorkspaceOperation::Updated),
    ]
    .into();
    let changes = diff_marks(&before, &after);
    assert_eq!(
        changes.changes,
        [
            TurnWorkspaceChange {
                relative_path: "dirty.rs".into(),
                operation: TurnWorkspaceOperation::Deleted,
            },
            TurnWorkspaceChange {
                relative_path: "clean.rs".into(),
                operation: TurnWorkspaceOperation::Updated,
            },
        ]
    );
    // Marks stored before HEAD was recorded still deserialize.
    let legacy: WorkspaceMark =
        serde_json::from_str(r#"{"root_identity":null,"entries":{},"truncated":false}"#).unwrap();
    assert_eq!(legacy.head, None);
}

#[test]
fn only_paths_touched_during_the_turn_are_changes() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let before = mark(&[
        ("dirty-untouched.rs", " M", Some(10), 1),
        ("dirty-edited.rs", " M", Some(10), 1),
        ("reverted.rs", " M", Some(4), 1),
    ]);
    let after = mark(&[
        ("dirty-untouched.rs", " M", Some(10), 1),
        ("dirty-edited.rs", " M", Some(12), 2),
        ("README.md", " M", Some(20), 3),
        ("new.txt", "??", Some(1), 3),
        ("gone.rs", " D", None, 0),
    ]);
    let changes = diff_marks(&before, &after);
    assert!(changes.tracked && !changes.truncated);
    let by_path: BTreeMap<_, _> = changes
        .changes
        .iter()
        .map(|change| (change.relative_path.as_str(), change.operation))
        .collect();
    assert_eq!(by_path.len(), 5);
    assert!(!by_path.contains_key("dirty-untouched.rs"));
    assert_eq!(by_path["README.md"], TurnWorkspaceOperation::Updated);
    assert_eq!(by_path["dirty-edited.rs"], TurnWorkspaceOperation::Updated);
    assert_eq!(by_path["new.txt"], TurnWorkspaceOperation::Created);
    assert_eq!(by_path["gone.rs"], TurnWorkspaceOperation::Deleted);
    assert_eq!(by_path["reverted.rs"], TurnWorkspaceOperation::Updated);
}

#[test]
fn a_shell_edit_in_a_real_repository_is_a_turn_change() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    let git = |args: &[&str]| {
        assert!(
            crate::test_git::command()
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "fixture"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(root.join("README.md"), b"hello\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-q", "-m", "init"]);
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let before = capture(&root).await.expect("a repository root has a mark");
        // What a harness shell command (for example Add-Content) does.
        std::fs::write(root.join("README.md"), b"hello\nAudited.\n").unwrap();
        let after = capture(&root).await.unwrap();
        let changes = diff_marks(&before, &after);
        assert_eq!(
            changes.changes,
            [TurnWorkspaceChange {
                relative_path: "README.md".into(),
                operation: TurnWorkspaceOperation::Updated,
            }]
        );
    });
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn capture_skips_dependencies_and_ignored_directories_and_fails_closed_at_the_walk_limit() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    assert!(
        crate::test_git::command()
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    for directory in [
        "Node_Modules/pkg",
        "nested/node_modules/pkg",
        "nested/.GIT/objects",
        "ignored/build",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
        for index in 0..20 {
            std::fs::write(root.join(directory).join(format!("f{index}")), "skip").unwrap();
        }
    }
    std::fs::write(root.join("visible.txt"), "keep").unwrap();
    let root = root.canonicalize().unwrap();
    tauri::async_runtime::block_on(async {
        let mark = capture::capture_with_limit(&root, 10).await.unwrap();
        assert_eq!(
            mark.entries.keys().map(String::as_str).collect::<Vec<_>>(),
            [".gitignore", "visible.txt"]
        );
        assert!(capture::capture_with_limit(&root, 2).await.is_some());
        assert!(capture::capture_with_limit(&root, 1).await.is_none());
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn truncated_snapshots_do_not_invent_deleted_or_attributed_files() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let mut before = mark(&[("a.txt", " M", Some(1), 1)]);
    before.truncated = true;
    let changes = diff_marks(&before, &mark(&[]));
    assert!(!changes.tracked && changes.truncated && changes.changes.is_empty());
}

#[test]
fn replacing_the_root_at_the_same_path_never_attributes_the_replacement() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let before = mark(&[("first.txt", "??", Some(1), 1)]);
    let mut after = mark(&[("other.txt", "??", Some(1), 2)]);
    after.root_identity = Some([2; 24]);
    let changes = diff_marks(&before, &after);
    assert!(!changes.tracked && changes.changes.is_empty());
}

#[test]
fn deleting_a_tracked_directory_preserves_its_deletions_and_unrelated_edits() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = temp_root();
    let git = |args: &[&str]| {
        assert!(
            crate::test_git::command()
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "-q"]);
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/file.txt"), "Tracked child").unwrap();
    std::fs::write(root.join("keep.txt"), "Keep").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=fixture",
        "commit",
        "-q",
        "-m",
        "Initial",
    ]);
    let root = root.canonicalize().unwrap();
    let before = tauri::async_runtime::block_on(capture(&root)).unwrap();
    std::fs::remove_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("keep.txt"), "Edited unrelated file").unwrap();
    let after = tauri::async_runtime::block_on(capture(&root)).unwrap();
    let changes = diff_marks(&before, &after);
    assert!(changes.tracked);
    assert_eq!(
        changes.changes,
        [
            TurnWorkspaceChange {
                relative_path: "keep.txt".to_owned(),
                operation: TurnWorkspaceOperation::Updated
            },
            TurnWorkspaceChange {
                relative_path: "sub/file.txt".to_owned(),
                operation: TurnWorkspaceOperation::Deleted
            }
        ]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn non_git_baseline_is_unsupported_while_unreadable_root_is_failed() {
    let root = temp_root();
    tauri::async_runtime::block_on(async {
        assert!(matches!(
            capture_baseline(&root).await,
            BaselineCapture::Unsupported
        ));
        assert!(matches!(
            capture_baseline(&root.join("missing")).await,
            BaselineCapture::Failed
        ));
        std::fs::write(root.join(".git"), "invalid Git metadata").unwrap();
        assert!(matches!(
            capture_baseline(&root).await,
            BaselineCapture::Failed
        ));
    });
    std::fs::remove_dir_all(root).unwrap();
}
