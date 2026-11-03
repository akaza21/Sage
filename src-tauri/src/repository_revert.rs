use crate::{
    error::{AppError, AppResult},
    git_budget, repository, repository_conflict,
    workspace::Workspace,
};

const MAX_GIT_MUTATION_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) async fn continue_revert(workspace: &Workspace) -> AppResult<()> {
    continue_revert_with_policy(
        workspace,
        repository::GitProcessPolicy::mutation(git_budget::MUTATION),
    )
    .await
}

pub(crate) async fn continue_revert_with_policy(
    workspace: &Workspace,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Continue revert requires a Project root that equals the repository top level".into(),
        ));
    }
    if !repository_conflict::revert_in_progress(workspace).await? {
        return Err(AppError::InvalidRequest(
            "Continue revert requires an in-progress revert".into(),
        ));
    }
    if repository_conflict::merge_in_progress(workspace).await?
        || repository_conflict::rebase_in_progress(workspace).await?
        || repository_conflict::cherry_pick_in_progress(workspace).await?
    {
        return Err(AppError::InvalidRequest(
            "Continue revert cannot run during an in-progress merge, rebase, or cherry-pick".into(),
        ));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "Continue revert requires resolved conflicts".into(),
        ));
    }
    let editor_command = repository_conflict::NOOP_GIT_EDITOR;
    let editor = format!("core.editor={editor_command}");
    let sequence = format!("sequence.editor={editor_command}");
    let output = repository::run_git_with_policy(
        workspace.root(),
        &[
            "-c",
            editor.as_str(),
            "-c",
            sequence.as_str(),
            "-c",
            "commit.gpgSign=false",
            "revert",
            "--continue",
        ],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use uuid::Uuid;

    use super::*;
    use crate::domain::{FileChange, GitFileMutationAction, ResourceRef};
    use crate::workspace::Workspace;

    fn git(root: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
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

    fn revert_conflict_fixture() -> (std::path::PathBuf, Workspace, ResourceRef) {
        let root = std::env::temp_dir().join(format!("sage-git-revert-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("conflict.txt"), "base\n").unwrap();
        git(&root, &["add", "--", "conflict.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        fs::write(root.join("conflict.txt"), "reverted\n").unwrap();
        fs::write(root.join("extra.txt"), "extra\n").unwrap();
        git(&root, &["add", "--", "conflict.txt", "extra.txt"]);
        git(&root, &["commit", "-q", "-m", "to-revert"]);
        fs::write(root.join("conflict.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "later"]);
        let revert = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["revert", "--no-edit", "HEAD~1"])
            .output()
            .unwrap();
        assert!(!revert.status.success(), "expected a revert conflict");
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("conflict.txt").unwrap();
        (root, workspace, resource)
    }

    #[test]
    fn revert_in_progress_is_true_only_for_revert_head() {
        let (root, workspace, _) = revert_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(
            runtime
                .block_on(repository_conflict::revert_in_progress(&workspace))
                .unwrap()
        );
        assert!(
            !runtime
                .block_on(repository_conflict::merge_in_progress(&workspace))
                .unwrap()
        );
        assert!(
            !runtime
                .block_on(repository_conflict::rebase_in_progress(&workspace))
                .unwrap()
        );
        assert!(
            !runtime
                .block_on(repository_conflict::cherry_pick_in_progress(&workspace))
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keep_ours_resolves_a_revert_conflict_to_the_current_commit() {
        let (root, workspace, resource) = revert_conflict_fixture();
        let change = conflicted("conflict.txt");
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(repository_conflict::resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(crate::test_wait::BUDGET),
            ))
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_revert_completes_after_keep_ours() {
        let (root, workspace, resource) = revert_conflict_fixture();
        let change = conflicted("conflict.txt");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(repository_conflict::resolve_conflicted_file(
                &workspace,
                &resource,
                &change,
                GitFileMutationAction::KeepOurs,
                repository::GitProcessPolicy::mutation(crate::test_wait::BUDGET),
            ))
            .unwrap();
        runtime.block_on(continue_revert(&workspace)).unwrap();
        assert!(
            !runtime
                .block_on(repository_conflict::revert_in_progress(&workspace))
                .unwrap()
        );
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_revert_rejects_unmerged_paths_and_idle_repositories() {
        let (root, workspace, _) = revert_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let unmerged = runtime.block_on(continue_revert(&workspace)).unwrap_err();
        assert!(unmerged.to_string().contains("resolved conflicts"));
        fs::remove_dir_all(root).unwrap();

        let idle = std::env::temp_dir().join(format!("sage-git-norevert-{}", Uuid::new_v4()));
        fs::create_dir_all(&idle).unwrap();
        git(&idle, &["init", "-q"]);
        git(&idle, &["config", "user.email", "sage@example.invalid"]);
        git(&idle, &["config", "user.name", "Sage Test"]);
        fs::write(idle.join("ok.txt"), "ok\n").unwrap();
        git(&idle, &["add", "--", "ok.txt"]);
        git(&idle, &["commit", "-q", "-m", "ok"]);
        let idle_workspace = Workspace::open("project-a", "root-a", &idle).unwrap();
        let idle_error = runtime
            .block_on(continue_revert(&idle_workspace))
            .unwrap_err();
        assert!(idle_error.to_string().contains("in-progress revert"));
        fs::remove_dir_all(idle).unwrap();
    }
}
