use super::*;

fn temp_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sage-turn-workspace-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn porcelain_records_become_a_bounded_fingerprint() {
    let root = temp_root();
    std::fs::write(root.join("a.txt"), b"one").unwrap();
    let mark = parse_mark(&root, b" M a.txt\0?? new.txt\0bad\0", false);
    assert_eq!(mark.entries.len(), 2);
    assert_eq!(mark.entries["a.txt"].len, Some(3));
    assert_eq!(mark.entries["new.txt"].len, None);
    let many: Vec<u8> = (0..MAX_TURN_WORKSPACE_CHANGES + 5)
        .flat_map(|index| format!("?? f{index}\0").into_bytes())
        .collect();
    let bounded = parse_mark(&root, &many, false);
    assert_eq!(bounded.entries.len(), MAX_TURN_WORKSPACE_CHANGES);
    assert!(bounded.truncated);
    std::fs::remove_dir_all(root).unwrap();
}

fn mark(entries: &[(&str, &str, Option<u64>, u128)]) -> WorkspaceMark {
    WorkspaceMark {
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
fn a_turn_settles_once_and_unknown_turns_are_untracked() {
    let key = format!("turn-{}", uuid::Uuid::new_v4());
    assert_eq!(settle(&key, None), TurnWorkspaceChanges::untracked());
    remember(key.clone(), mark(&[]));
    assert_eq!(is_settled(&key), Some(false));
    let first = settle(&key, Some(mark(&[("x.rs", " M", Some(1), 1)])));
    assert_eq!(first.changes.len(), 1);
    // A later capture never rewrites the settled answer.
    assert_eq!(settle(&key, Some(mark(&[]))), first);
    assert_eq!(is_settled(&key), Some(true));
}

#[test]
fn a_shell_edit_in_a_real_repository_is_a_turn_change() {
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
