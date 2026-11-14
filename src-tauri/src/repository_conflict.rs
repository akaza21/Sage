use std::path::PathBuf;

use sage_core::error::{AppError, AppResult};

use crate::repository;
use sage_domain::{
    file_operations::GitFileMutationAction, repository::FileChange, resource::ResourceRef,
};
use sage_workspace::Workspace;

const MAX_GIT_ADMIN_PATH_BYTES: usize = 32 * 1024;
const MAX_GIT_MUTATION_OUTPUT_BYTES: usize = 64 * 1024;
pub(crate) const NOOP_GIT_EDITOR: &str = ":";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictSide {
    Ours,
    Theirs,
}

pub(crate) fn admit_conflict_resolution(
    change: &FileChange,
    action: GitFileMutationAction,
) -> AppResult<ConflictSide> {
    let side = match action {
        GitFileMutationAction::KeepOurs => ConflictSide::Ours,
        GitFileMutationAction::KeepTheirs => ConflictSide::Theirs,
        _ => {
            return Err(AppError::InvalidRequest(
                "conflict resolution requires Keep ours or Keep theirs".into(),
            ));
        }
    };
    if change.index_status != "conflicted" && change.worktree_status != "conflicted" {
        return Err(AppError::InvalidRequest(
            "conflict resolution requires a conflicted file".into(),
        ));
    }
    if change.original_resource.is_some() {
        return Err(AppError::InvalidRequest(
            "renamed conflicted files require the later merge workflow".into(),
        ));
    }
    Ok(side)
}

pub(crate) async fn resolve_conflicted_file(
    workspace: &Workspace,
    resource: &ResourceRef,
    change: &FileChange,
    action: GitFileMutationAction,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    let side = admit_conflict_resolution(change, action)?;
    require_in_progress_merge(workspace).await?;
    let flag = match side {
        ConflictSide::Ours => "--ours",
        ConflictSide::Theirs => "--theirs",
    };
    let checkout = repository::run_git_with_policy(
        workspace.root(),
        &["checkout", flag, "--", resource.relative_path.as_str()],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy.clone(),
    )
    .await?;
    repository::ensure_success(&checkout)?;
    let add = repository::run_git_with_policy(
        workspace.root(),
        &["add", "--", resource.relative_path.as_str()],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&add)
}

pub(crate) async fn continue_rebase(workspace: &Workspace) -> AppResult<()> {
    continue_rebase_with_policy(
        workspace,
        repository::GitProcessPolicy::mutation(std::time::Duration::from_secs(3)),
    )
    .await
}

pub(crate) async fn continue_rebase_with_policy(
    workspace: &Workspace,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Continue rebase requires a Project root that equals the repository top level".into(),
        ));
    }
    if !rebase_in_progress(workspace).await? {
        return Err(AppError::InvalidRequest(
            "Continue rebase requires an in-progress rebase".into(),
        ));
    }
    if merge_in_progress(workspace).await? {
        return Err(AppError::InvalidRequest(
            "Continue rebase cannot run during an in-progress merge".into(),
        ));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "Continue rebase requires resolved conflicts".into(),
        ));
    }
    let editor = format!("core.editor={NOOP_GIT_EDITOR}");
    let sequence = format!("sequence.editor={NOOP_GIT_EDITOR}");
    let output = repository::run_git_with_policy(
        workspace.root(),
        &[
            "-c",
            editor.as_str(),
            "-c",
            sequence.as_str(),
            "-c",
            "commit.gpgSign=false",
            "rebase",
            "--continue",
        ],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)
}

pub(crate) async fn continue_cherry_pick(workspace: &Workspace) -> AppResult<()> {
    continue_cherry_pick_with_policy(
        workspace,
        repository::GitProcessPolicy::mutation(std::time::Duration::from_secs(3)),
    )
    .await
}

pub(crate) async fn continue_cherry_pick_with_policy(
    workspace: &Workspace,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Continue cherry-pick requires a Project root that equals the repository top level"
                .into(),
        ));
    }
    if !cherry_pick_in_progress(workspace).await? {
        return Err(AppError::InvalidRequest(
            "Continue cherry-pick requires an in-progress cherry-pick".into(),
        ));
    }
    if merge_in_progress(workspace).await? || rebase_in_progress(workspace).await? {
        return Err(AppError::InvalidRequest(
            "Continue cherry-pick cannot run during an in-progress merge or rebase".into(),
        ));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "Continue cherry-pick requires resolved conflicts".into(),
        ));
    }
    let editor = format!("core.editor={NOOP_GIT_EDITOR}");
    let sequence = format!("sequence.editor={NOOP_GIT_EDITOR}");
    let output = repository::run_git_with_policy(
        workspace.root(),
        &[
            "-c",
            editor.as_str(),
            "-c",
            sequence.as_str(),
            "-c",
            "commit.gpgSign=false",
            "cherry-pick",
            "--continue",
        ],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)
}

async fn exclusive_admin_markers(
    workspace: &Workspace,
    required: &[&str],
    forbidden: &[&str],
) -> AppResult<bool> {
    let mut any_required = false;
    for marker in required {
        if admin_marker_exists(workspace, marker).await? {
            any_required = true;
        }
    }
    if !any_required {
        return Ok(false);
    }
    for marker in forbidden {
        if admin_marker_exists(workspace, marker).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) async fn merge_in_progress(workspace: &Workspace) -> AppResult<bool> {
    exclusive_admin_markers(
        workspace,
        &["MERGE_HEAD"],
        &[
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "REBASE_HEAD",
            "rebase-merge",
            "rebase-apply",
        ],
    )
    .await
}

pub(crate) async fn rebase_in_progress(workspace: &Workspace) -> AppResult<bool> {
    exclusive_admin_markers(
        workspace,
        &["rebase-merge", "rebase-apply"],
        &["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"],
    )
    .await
}

pub(crate) async fn cherry_pick_in_progress(workspace: &Workspace) -> AppResult<bool> {
    exclusive_admin_markers(
        workspace,
        &["CHERRY_PICK_HEAD"],
        &[
            "MERGE_HEAD",
            "REVERT_HEAD",
            "REBASE_HEAD",
            "rebase-merge",
            "rebase-apply",
        ],
    )
    .await
}

pub(crate) async fn revert_in_progress(workspace: &Workspace) -> AppResult<bool> {
    exclusive_admin_markers(
        workspace,
        &["REVERT_HEAD"],
        &[
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REBASE_HEAD",
            "rebase-merge",
            "rebase-apply",
        ],
    )
    .await
}

async fn require_in_progress_merge(workspace: &Workspace) -> AppResult<()> {
    if merge_in_progress(workspace).await?
        || rebase_in_progress(workspace).await?
        || cherry_pick_in_progress(workspace).await?
        || revert_in_progress(workspace).await?
    {
        return Ok(());
    }
    Err(AppError::InvalidRequest(
        "Keep ours and Keep theirs require an in-progress merge, rebase, cherry-pick, or revert"
            .into(),
    ))
}

pub(crate) async fn admin_marker_exists(workspace: &Workspace, marker: &str) -> AppResult<bool> {
    let output = crate::repository_workspace::run_git(
        workspace,
        &["rev-parse", "--path-format=absolute", "--git-path", marker],
        MAX_GIT_ADMIN_PATH_BYTES + 2,
    )
    .await?;
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git administrative path exceeds its bounded contract".into(),
        ));
    }
    let path = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git administrative path is not UTF-8".into()))?
        .trim_end_matches(['\r', '\n']);
    if path.is_empty() || path.contains(['\r', '\n', '\0']) {
        return Err(AppError::Git(
            "Git returned an invalid administrative path".into(),
        ));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(AppError::Git(
            "Git returned a non-absolute administrative path".into(),
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use uuid::Uuid;

    use std::time::Duration;

    use super::*;
    use sage_workspace::Workspace;

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

    fn conflicted(path: &str) -> FileChange {
        FileChange {
            resource: ResourceRef {
                root_id: "root-a".into(),
                relative_path: path.into(),
            },
            original_resource: None,
            index_status: "conflicted".into(),
            worktree_status: "conflicted".into(),
            additions: None,
            deletions: None,
        }
    }

    fn merge_conflict_fixture() -> (std::path::PathBuf, Workspace, ResourceRef) {
        let root = std::env::temp_dir().join(format!("sage-git-conflict-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("conflict.txt"), "base\n").unwrap();
        git(&root, &["add", "--", "conflict.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["checkout", "-q", "-b", "other"]);
        fs::write(root.join("conflict.txt"), "other\n").unwrap();
        git(&root, &["commit", "-q", "-am", "other"]);
        git(&root, &["checkout", "-q", "-"]);
        fs::write(root.join("conflict.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "ours"]);
        let merge = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["merge", "other"])
            .output()
            .unwrap();
        assert!(!merge.status.success());
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("conflict.txt").unwrap();
        (root, workspace, resource)
    }

    #[test]
    fn admit_keep_sides_only_for_conflicted_files() {
        let change = conflicted("conflict.txt");
        assert_eq!(
            admit_conflict_resolution(&change, GitFileMutationAction::KeepOurs).unwrap(),
            ConflictSide::Ours
        );
        assert_eq!(
            admit_conflict_resolution(&change, GitFileMutationAction::KeepTheirs).unwrap(),
            ConflictSide::Theirs
        );
        assert!(admit_conflict_resolution(&change, GitFileMutationAction::Discard).is_err());
        let modified = FileChange {
            index_status: "modified".into(),
            worktree_status: "unchanged".into(),
            ..change
        };
        assert!(admit_conflict_resolution(&modified, GitFileMutationAction::KeepOurs).is_err());
    }

    #[test]
    fn keep_ours_resolves_a_merge_conflict_to_the_current_branch() {
        let (root, workspace, resource) = merge_conflict_fixture();
        let change = conflicted("conflict.txt");
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        let unmerged = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["ls-files", "--unmerged", "-z"])
            .output()
            .unwrap();
        assert!(unmerged.stdout.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keep_theirs_resolves_a_merge_conflict_to_the_incoming_branch() {
        let (root, workspace, resource) = merge_conflict_fixture();
        let change = conflicted("conflict.txt");
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepTheirs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "other\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merge_in_progress_is_true_only_for_a_merge_head() {
        let (root, workspace, _) = merge_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(merge_in_progress(&workspace)).unwrap());
        fs::remove_dir_all(root).unwrap();

        let plain = std::env::temp_dir().join(format!("sage-git-nomerge-flag-{}", Uuid::new_v4()));
        fs::create_dir_all(&plain).unwrap();
        git(&plain, &["init", "-q"]);
        git(&plain, &["config", "user.email", "sage@example.invalid"]);
        git(&plain, &["config", "user.name", "Sage Test"]);
        fs::write(plain.join("ok.txt"), "ok\n").unwrap();
        git(&plain, &["add", "--", "ok.txt"]);
        git(&plain, &["commit", "-q", "-m", "ok"]);
        let plain_workspace = Workspace::open("project-a", "root-a", &plain).unwrap();
        assert!(
            !runtime
                .block_on(merge_in_progress(&plain_workspace))
                .unwrap()
        );
        fs::remove_dir_all(plain).unwrap();
    }

    fn current_branch(root: &Path) -> String {
        let output = crate::git_process::fixture_git()
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn rebase_conflict_fixture() -> (std::path::PathBuf, Workspace, ResourceRef) {
        let (root, workspace, resource) = merge_conflict_fixture();
        git(&root, &["merge", "--abort"]);
        let onto = current_branch(&root);
        git(&root, &["checkout", "-q", "other"]);
        let rebase = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["rebase", &onto])
            .output()
            .unwrap();
        assert!(!rebase.status.success(), "expected a rebase conflict");
        (root, workspace, resource)
    }

    #[test]
    fn rebase_in_progress_is_true_only_for_rebase_admin() {
        let (root, workspace, _) = rebase_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(rebase_in_progress(&workspace)).unwrap());
        assert!(!runtime.block_on(merge_in_progress(&workspace)).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keep_ours_resolves_a_rebase_conflict_to_the_onto_commit() {
        let (root, workspace, resource) = rebase_conflict_fixture();
        let change = conflicted("conflict.txt");
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_rebase_completes_after_keep_ours() {
        let (root, workspace, resource) = rebase_conflict_fixture();
        let change = conflicted("conflict.txt");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        runtime.block_on(continue_rebase(&workspace)).unwrap();
        assert!(!runtime.block_on(rebase_in_progress(&workspace)).unwrap());
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_rebase_preserves_the_commit_message_without_opening_an_editor() {
        // Repository editor keys are refused before Git runs, so no editor is
        // configured here: without Sage's no-op override Git would fall back
        // to its default editor.
        let (root, workspace, resource) = rebase_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &conflicted("conflict.txt"),
                GitFileMutationAction::KeepTheirs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        runtime.block_on(continue_rebase(&workspace)).unwrap();
        assert!(!runtime.block_on(rebase_in_progress(&workspace)).unwrap());
        assert_eq!(current_branch(&root), "other");
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "other\n"
        );
        let commit = runtime
            .block_on(repository::run_git(
                workspace.root(),
                &["log", "-1", "--format=%B"],
                MAX_GIT_MUTATION_OUTPUT_BYTES,
            ))
            .unwrap();
        repository::ensure_success(&commit).unwrap();
        assert_eq!(String::from_utf8(commit.stdout).unwrap().trim(), "other");
        fs::remove_dir_all(root).unwrap();
    }

    fn cherry_pick_conflict_fixture() -> (std::path::PathBuf, Workspace, ResourceRef) {
        let root = std::env::temp_dir().join(format!("sage-git-cherry-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("conflict.txt"), "base\n").unwrap();
        git(&root, &["add", "--", "conflict.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["checkout", "-q", "-b", "other"]);
        fs::write(root.join("conflict.txt"), "other\n").unwrap();
        fs::write(root.join("extra.txt"), "extra\n").unwrap();
        git(&root, &["add", "--", "conflict.txt", "extra.txt"]);
        git(&root, &["commit", "-q", "-m", "other"]);
        git(&root, &["checkout", "-q", "-"]);
        fs::write(root.join("conflict.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "ours"]);
        let cherry = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["cherry-pick", "other"])
            .output()
            .unwrap();
        assert!(!cherry.status.success(), "expected a cherry-pick conflict");
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("conflict.txt").unwrap();
        (root, workspace, resource)
    }

    #[test]
    fn cherry_pick_in_progress_is_true_only_for_cherry_pick_head() {
        let (root, workspace, _) = cherry_pick_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(
            runtime
                .block_on(cherry_pick_in_progress(&workspace))
                .unwrap()
        );
        assert!(!runtime.block_on(merge_in_progress(&workspace)).unwrap());
        assert!(!runtime.block_on(rebase_in_progress(&workspace)).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keep_ours_resolves_a_cherry_pick_conflict_to_the_current_commit() {
        let (root, workspace, resource) = cherry_pick_conflict_fixture();
        let change = conflicted("conflict.txt");
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_cherry_pick_completes_after_keep_ours() {
        let (root, workspace, resource) = cherry_pick_conflict_fixture();
        let change = conflicted("conflict.txt");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap();
        runtime.block_on(continue_cherry_pick(&workspace)).unwrap();
        assert!(
            !runtime
                .block_on(cherry_pick_in_progress(&workspace))
                .unwrap()
        );
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_cherry_pick_rejects_unmerged_paths_and_idle_repositories() {
        let (root, workspace, _) = cherry_pick_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let unmerged = runtime
            .block_on(continue_cherry_pick(&workspace))
            .unwrap_err();
        assert!(unmerged.to_string().contains("resolved conflicts"));
        fs::remove_dir_all(root).unwrap();

        let idle = std::env::temp_dir().join(format!("sage-git-nocherry-{}", Uuid::new_v4()));
        fs::create_dir_all(&idle).unwrap();
        git(&idle, &["init", "-q"]);
        git(&idle, &["config", "user.email", "sage@example.invalid"]);
        git(&idle, &["config", "user.name", "Sage Test"]);
        fs::write(idle.join("ok.txt"), "ok\n").unwrap();
        git(&idle, &["add", "--", "ok.txt"]);
        git(&idle, &["commit", "-q", "-m", "ok"]);
        let idle_workspace = Workspace::open("project-a", "root-a", &idle).unwrap();
        let idle_error = runtime
            .block_on(continue_cherry_pick(&idle_workspace))
            .unwrap_err();
        assert!(idle_error.to_string().contains("in-progress cherry-pick"));
        fs::remove_dir_all(idle).unwrap();
    }

    #[test]
    fn continue_rebase_rejects_unmerged_paths_and_idle_repositories() {
        let (root, workspace, _) = rebase_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let unmerged = runtime.block_on(continue_rebase(&workspace)).unwrap_err();
        assert!(unmerged.to_string().contains("resolved conflicts"));
        fs::remove_dir_all(root).unwrap();

        let idle = std::env::temp_dir().join(format!("sage-git-norebase-{}", Uuid::new_v4()));
        fs::create_dir_all(&idle).unwrap();
        git(&idle, &["init", "-q"]);
        git(&idle, &["config", "user.email", "sage@example.invalid"]);
        git(&idle, &["config", "user.name", "Sage Test"]);
        fs::write(idle.join("ok.txt"), "ok\n").unwrap();
        git(&idle, &["add", "--", "ok.txt"]);
        git(&idle, &["commit", "-q", "-m", "ok"]);
        let idle_workspace = Workspace::open("project-a", "root-a", &idle).unwrap();
        let idle_error = runtime
            .block_on(continue_rebase(&idle_workspace))
            .unwrap_err();
        assert!(idle_error.to_string().contains("in-progress rebase"));
        fs::remove_dir_all(idle).unwrap();
    }

    #[test]
    fn keep_ours_rejects_when_no_merge_is_in_progress() {
        let root = std::env::temp_dir().join(format!("sage-git-nomerge-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("ok.txt"), "ok\n").unwrap();
        git(&root, &["add", "--", "ok.txt"]);
        git(&root, &["commit", "-q", "-m", "ok"]);
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("ok.txt").unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(resolve_conflicted_file(
                &workspace,
                &resource,
                &conflicted("ok.txt"),
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(Duration::from_secs(3)),
            ))
            .unwrap_err();
        assert!(error.to_string().contains("in-progress merge"));
        fs::remove_dir_all(root).unwrap();
    }
}
