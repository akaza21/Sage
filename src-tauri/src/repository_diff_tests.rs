use std::{fs, path::Path};

use sage_core::error::AppError;

use super::{GitDiffScope, scoped_file_diff};
use crate::{domain::FileDiff, workspace::Workspace};

struct Repository(std::path::PathBuf);

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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

fn fixture() -> (Repository, Workspace) {
    let root = std::env::temp_dir().join(format!("sage-scoped-diff-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "fixture"]);
    git(&root, &["config", "user.name", "Sage Test"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
    (Repository(root), workspace)
}

fn text(diff: &FileDiff) -> String {
    diff.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn scoped_diffs_refuse_repository_textconv_and_show_raw_content() {
    let (repo, workspace) = fixture();
    fs::write(repo.0.join(".gitattributes"), "*.txt diff=sentinel\n").unwrap();
    fs::write(repo.0.join("tracked.txt"), "base\n").unwrap();
    git(&repo.0, &["add", "."]);
    git(&repo.0, &["commit", "-q", "-m", "fixture"]);
    git(
        &repo.0,
        &["config", "diff.sentinel.textconv", "echo converted"],
    );
    fs::write(repo.0.join("tracked.txt"), "indexed\n").unwrap();
    git(&repo.0, &["add", "tracked.txt"]);
    fs::write(repo.0.join("tracked.txt"), "working\n").unwrap();
    let resource = workspace.resource("tracked.txt").unwrap();
    for scope in [
        None,
        Some(GitDiffScope::Staged),
        Some(GitDiffScope::Unstaged),
    ] {
        let error = scoped_file_diff(&workspace, &resource, scope)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, AppError::UnsafeRepositoryConfig(message)
                if message.contains("diff.sentinel.textconv")),
            "{error}"
        );
    }

    git(&repo.0, &["config", "--unset", "diff.sentinel.textconv"]);
    for (scope, removed, added) in [
        (None, "base", "working"),
        (Some(GitDiffScope::Staged), "base", "indexed"),
        (Some(GitDiffScope::Unstaged), "indexed", "working"),
    ] {
        let diff = scoped_file_diff(&workspace, &resource, scope)
            .await
            .unwrap();
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines[0].kind, "removed");
        assert_eq!(diff.hunks[0].lines[0].text, removed);
        assert_eq!(diff.hunks[0].lines[1].kind, "added");
        assert_eq!(diff.hunks[0].lines[1].text, added);
    }
}

#[tokio::test]
async fn staged_content_excludes_a_later_external_edit_even_with_cached_status() {
    let (repo, workspace) = fixture();
    fs::write(repo.0.join("tracked.txt"), "base\n").unwrap();
    let resource = workspace.resource("tracked.txt").unwrap();
    git(&repo.0, &["add", "tracked.txt"]);
    git(&repo.0, &["commit", "-q", "-m", "fixture"]);
    fs::write(repo.0.join("tracked.txt"), "staged\n").unwrap();
    git(&repo.0, &["add", "tracked.txt"]);
    let cached = crate::repository::status(&workspace).await.unwrap();
    assert_eq!(cached.changes.len(), 1);
    fs::write(repo.0.join("tracked.txt"), "external-only\n").unwrap();
    let staged = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Staged))
        .await
        .unwrap();
    let unstaged = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Unstaged))
        .await
        .unwrap();
    assert!(text(&staged).contains("staged"));
    assert!(!text(&staged).contains("external-only"));
    assert!(!text(&unstaged).contains("base"));
    assert!(text(&unstaged).contains("external-only"));
}

#[tokio::test]
async fn unborn_index_and_untracked_content_remain_separate() {
    let (repo, workspace) = fixture();
    fs::write(repo.0.join("new.txt"), "indexed\n").unwrap();
    let resource = workspace.resource("new.txt").unwrap();
    git(&repo.0, &["add", "new.txt"]);
    fs::write(repo.0.join("new.txt"), "working\n").unwrap();
    let staged = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Staged))
        .await
        .unwrap();
    let unstaged = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Unstaged))
        .await
        .unwrap();
    assert!(text(&staged).contains("indexed"));
    assert!(!text(&staged).contains("working"));
    assert!(text(&unstaged).contains("working"));
    fs::write(repo.0.join("untracked.txt"), "untracked\n").unwrap();
    let untracked = workspace.resource("untracked.txt").unwrap();
    assert!(
        scoped_file_diff(&workspace, &untracked, Some(GitDiffScope::Staged))
            .await
            .unwrap()
            .hunks
            .is_empty()
    );
    assert!(
        text(
            &scoped_file_diff(&workspace, &untracked, Some(GitDiffScope::Unstaged))
                .await
                .unwrap()
        )
        .contains("untracked")
    );
}

#[tokio::test]
async fn deleted_worktree_files_keep_their_staged_and_unstaged_content() {
    let (repo, workspace) = fixture();
    fs::write(repo.0.join("deleted.txt"), "committed\n").unwrap();
    let resource = workspace.resource("deleted.txt").unwrap();
    git(&repo.0, &["add", "deleted.txt"]);
    git(&repo.0, &["commit", "-q", "-m", "fixture"]);
    fs::write(repo.0.join("deleted.txt"), "indexed\n").unwrap();
    git(&repo.0, &["add", "deleted.txt"]);
    fs::remove_file(repo.0.join("deleted.txt")).unwrap();
    let staged = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Staged))
        .await
        .unwrap();
    let unstaged = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Unstaged))
        .await
        .unwrap();
    assert!(text(&staged).contains("committed"));
    assert!(text(&staged).contains("indexed"));
    assert!(!text(&unstaged).contains("committed"));
    assert!(text(&unstaged).contains("indexed"));
    git(&repo.0, &["add", "-u"]);
    let deleted = scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Staged))
        .await
        .unwrap();
    assert!(text(&deleted).contains("committed"));
    assert!(!text(&deleted).contains("indexed"));
    assert!(
        scoped_file_diff(&workspace, &resource, Some(GitDiffScope::Unstaged))
            .await
            .unwrap()
            .hunks
            .is_empty()
    );
}
