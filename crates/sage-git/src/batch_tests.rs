use std::{fs, path::Path};

use super::*;
use crate::process::GitCommandCounts;

fn git(root: &Path, args: &[&str]) {
    assert!(
        crate::process::fixture_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn git_stdout(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = crate::process::fixture_git()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    output.stdout
}

#[test]
fn batch_unstage_preserves_clean_index_stat_cache() {
    use std::io::Write;

    let root = std::env::temp_dir().join(format!("sage-git-batch-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    // Isolate reset's refresh from the preflight numstat diff's own refresh.
    git(&root, &["config", "diff.autoRefreshIndex", "false"]);
    let clean_paths: Vec<_> = (0..64)
        .map(|index| format!("clean-{index:03}.txt"))
        .collect();
    for path in clean_paths
        .iter()
        .map(String::as_str)
        .chain(["literal[ab].txt", "literala.txt"])
    {
        fs::write(root.join(path), "base\n").unwrap();
    }
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture",
            "commit",
            "-qm",
            "base",
        ],
    );
    for path in ["literal[ab].txt", "literala.txt"] {
        fs::write(root.join(path), "changed\n").unwrap();
    }
    git(&root, &["add", "."]);

    // Seed clean entries with empty stat caches, without relying on clock ticks
    // or filesystem timestamp precision. A refreshing reset repopulates them.
    let oid = String::from_utf8(git_stdout(&root, &["rev-parse", "HEAD:clean-000.txt"])).unwrap();
    let mut child = crate::process::fixture_git()
        .arg("-C")
        .arg(&root)
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        for path in &clean_paths {
            writeln!(input, "100644 {}\t{path}", oid.trim()).unwrap();
        }
    }
    assert!(child.wait().unwrap().success());
    let before = git_stdout(&root, &["ls-files", "--debug", "--", "clean-*.txt"]);
    assert_eq!(
        String::from_utf8_lossy(&before)
            .matches("mtime: 0:0")
            .count(),
        clean_paths.len()
    );

    let workspace = Workspace::open("project", "root", &root).unwrap();
    let counts = GitCommandCounts::observe(&root);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        mutate_paths(
            &workspace,
            &[workspace.resource("literal[ab].txt").unwrap()],
            GitFileMutationAction::Unstage,
        )
        .await
        .unwrap();
    });
    assert_eq!(counts.count("status"), 1);
    assert_eq!(counts.count("reset"), 1);
    assert_eq!(
        git_stdout(&root, &["ls-files", "--debug", "--", "clean-*.txt"]),
        before,
        "batch unstage must not refresh unrelated clean index entries"
    );
    assert_eq!(
        git_stdout(&root, &["diff", "--cached", "--name-only"]),
        b"literala.txt\n"
    );
    assert_eq!(git_stdout(&root, &["show", ":literal[ab].txt"]), b"base\n");
    assert_eq!(
        fs::read_to_string(root.join("literal[ab].txt")).unwrap(),
        "changed\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn batch_handles_300_long_literal_paths_beyond_windows_command_line_limit() {
    let root = std::env::temp_dir().join(format!("sage-git-batch-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let paths: Vec<_> = (0..300)
        .map(|index| format!("file-{index:03}-{}[ab].txt", "x".repeat(103)))
        .collect();
    for path in &paths {
        fs::write(root.join(path), "base\n").unwrap();
    }
    let workspace = Workspace::open("project", "root", &root).unwrap();
    let selected: Vec<_> = paths
        .iter()
        .map(|path| workspace.resource(path).unwrap())
        .collect();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        for action in [GitFileMutationAction::Stage, GitFileMutationAction::Unstage] {
            mutate_paths(&workspace, &selected, action).await.unwrap();
            let status = crate::status(&workspace).await.unwrap();
            assert_eq!(status.changes.len(), 300);
            assert!(status.changes.iter().all(|change| match action {
                GitFileMutationAction::Stage => change.index_status == "added",
                _ => change.worktree_status == "untracked" && change.index_status == "unchanged",
            }));
        }
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture",
                "commit",
                "-qm",
                "base",
            ],
        );
        for path in &paths {
            fs::write(root.join(path), "changed\n").unwrap();
        }
        for action in [
            GitFileMutationAction::Stage,
            GitFileMutationAction::Unstage,
            GitFileMutationAction::Discard,
        ] {
            let counts = GitCommandCounts::observe(&root);
            mutate_paths(&workspace, &selected, action).await.unwrap();
            assert_eq!(counts.count("status"), 1);
            assert_eq!(
                counts.count("add") + counts.count("reset") + counts.count("restore"),
                1
            );
            let status = crate::status(&workspace).await.unwrap();
            if action == GitFileMutationAction::Discard {
                assert!(status.changes.is_empty());
                for path in &paths {
                    assert_eq!(fs::read_to_string(root.join(path)).unwrap(), "base\n");
                }
            } else {
                assert_eq!(status.changes.len(), 300);
                assert!(status.changes.iter().all(|change| match action {
                    GitFileMutationAction::Stage =>
                        change.index_status == "modified" && change.worktree_status == "unchanged",
                    _ => change.index_status == "unchanged" && change.worktree_status == "modified",
                }));
            }
        }
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn batch_preserves_literal_paths_untracked_files_and_unborn_unstage() {
    let root = std::env::temp_dir().join(format!("sage-git-batch-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    for path in ["literal[ab].txt", "literala.txt"] {
        fs::write(root.join(path), "base\n").unwrap();
    }
    let workspace = Workspace::open("project", "root", &root).unwrap();
    let selected = [workspace.resource("literal[ab].txt").unwrap()];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        mutate_paths(&workspace, &selected, GitFileMutationAction::Stage)
            .await
            .unwrap();
        let status = crate::status(&workspace).await.unwrap();
        assert_eq!(
            status
                .changes
                .iter()
                .find(|change| change.resource.relative_path == "literal[ab].txt")
                .unwrap()
                .index_status,
            "added"
        );
        assert_eq!(
            status
                .changes
                .iter()
                .find(|change| change.resource.relative_path == "literala.txt")
                .unwrap()
                .worktree_status,
            "untracked"
        );
        mutate_paths(&workspace, &selected, GitFileMutationAction::Unstage)
            .await
            .unwrap();
        assert!(
            crate::status(&workspace)
                .await
                .unwrap()
                .changes
                .iter()
                .all(|change| change.worktree_status == "untracked")
        );
        assert!(
            mutate_paths(&workspace, &selected, GitFileMutationAction::Discard)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.join("literal[ab].txt")).unwrap(),
            "base\n"
        );
        git(&root, &["add", "--", "literal[ab].txt"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture",
                "commit",
                "-qm",
                "base",
            ],
        );
        fs::write(root.join("literal[ab].txt"), "staged\n").unwrap();
        mutate_paths(&workspace, &selected, GitFileMutationAction::Stage)
            .await
            .unwrap();
        fs::write(root.join("literal[ab].txt"), "unstaged\n").unwrap();
        mutate_paths(&workspace, &selected, GitFileMutationAction::Discard)
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("literal[ab].txt")).unwrap(),
            "staged\n"
        );
        let status = crate::status(&workspace).await.unwrap();
        let change = status
            .changes
            .iter()
            .find(|change| change.resource == selected[0])
            .unwrap();
        assert_eq!(
            (&*change.index_status, &*change.worktree_status),
            ("modified", "unchanged")
        );
        assert_eq!(
            fs::read_to_string(root.join("literala.txt")).unwrap(),
            "base\n"
        );
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_or_conflicted_selection_never_partially_stages_other_paths() {
    let root = std::env::temp_dir().join(format!("sage-git-batch-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    fs::write(root.join("conflict.txt"), "base\n").unwrap();
    git(&root, &["add", "--", "conflict.txt"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture",
            "commit",
            "-qm",
            "base",
        ],
    );
    // Create a real unmerged index without depending on branch names or a merge driver.
    let oid = crate::process::fixture_git()
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD:conflict.txt"])
        .output()
        .unwrap();
    let oid = String::from_utf8(oid.stdout).unwrap();
    git(
        &root,
        &["update-index", "--force-remove", "--", "conflict.txt"],
    );
    use std::io::Write;
    let mut child = crate::process::fixture_git()
        .arg("-C")
        .arg(&root)
        .arg("update-index")
        .arg("--index-info")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        child.stdin.take().unwrap(),
        "100644 {} 1\tconflict.txt\n100644 {} 2\tconflict.txt\n100644 {} 3\tconflict.txt\n",
        oid.trim(),
        oid.trim(),
        oid.trim()
    )
    .unwrap();
    assert!(child.wait().unwrap().success());
    fs::write(root.join("safe.txt"), "safe\n").unwrap();
    let workspace = Workspace::open("project", "root", &root).unwrap();
    let safe = workspace.resource("safe.txt").unwrap();
    let conflict = workspace.resource("conflict.txt").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let counts = GitCommandCounts::observe(&root);
        for action in [
            GitFileMutationAction::Stage,
            GitFileMutationAction::Unstage,
            GitFileMutationAction::Discard,
        ] {
            let error = mutate_paths(&workspace, &[conflict.clone(), safe.clone()], action)
                .await
                .unwrap_err();
            assert!(error.to_string().contains("conflicted"));
        }
        for resources in [
            vec![],
            vec![safe.clone(); 301],
            vec![ResourceRef {
                root_id: "foreign".into(),
                relative_path: "safe.txt".into(),
            }],
            vec![
                safe.clone(),
                ResourceRef {
                    root_id: "root".into(),
                    relative_path: "absent.txt".into(),
                },
            ],
        ] {
            assert!(
                mutate_paths(&workspace, &resources, GitFileMutationAction::Stage)
                    .await
                    .is_err()
            );
        }
        assert_eq!(counts.count("add"), 0);
        assert_eq!(counts.count("restore"), 0);
        let status = crate::status(&workspace).await.unwrap();
        assert_eq!(
            status
                .changes
                .iter()
                .find(|change| change.resource == safe)
                .unwrap()
                .worktree_status,
            "untracked"
        );
        assert!(
            status
                .changes
                .iter()
                .any(|change| change.resource == conflict && change.index_status == "conflicted")
        );
    });
    fs::remove_dir_all(root).unwrap();
}
