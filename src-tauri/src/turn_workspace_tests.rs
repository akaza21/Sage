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
    }
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
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@example.invalid"]);
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
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    for directory in [
        "node_modules/pkg",
        "nested/node_modules/pkg",
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
        assert!(capture::capture_with_limit(&root, 2).await.is_none());
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
            std::process::Command::new("git")
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
        "user.email=test@example.invalid",
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
