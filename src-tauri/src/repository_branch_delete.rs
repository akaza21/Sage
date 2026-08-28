use std::time::Duration;

use crate::{
    error::{AppError, AppResult},
    repository,
    repository_branches::{
        LocalBranchList, MAX_BRANCH_NAME_BYTES, list_local_branches, named_branch,
        valid_branch_name,
    },
    workspace::Workspace,
};

const GIT_DELETE_BRANCH_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_GIT_DELETE_BRANCH_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) fn admit_listed_local_branch_delete<'a>(
    listed: &'a LocalBranchList,
    current: Option<&str>,
    requested: &str,
) -> AppResult<&'a str> {
    if requested.is_empty()
        || requested.len() > MAX_BRANCH_NAME_BYTES
        || !valid_branch_name(requested)
    {
        return Err(AppError::InvalidRequest(
            "Git delete branch identity exceeds its bounded contract".into(),
        ));
    }
    if listed.truncated {
        return Err(AppError::InvalidRequest(
            "Git delete branch requires a complete local branch list".into(),
        ));
    }
    if named_branch(current) == Some(requested) {
        return Err(AppError::InvalidRequest(
            "Git delete branch requires a different listed local branch".into(),
        ));
    }
    listed
        .names
        .iter()
        .find(|name| name.as_str() == requested)
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Git delete branch requires an exact listed local branch".into(),
            )
        })
}

pub(crate) async fn delete_listed_local_branch(
    workspace: &Workspace,
    requested: &str,
) -> AppResult<()> {
    delete_listed_local_branch_with_policy(
        workspace,
        requested,
        repository::GitProcessPolicy::mutation(GIT_DELETE_BRANCH_TIMEOUT),
    )
    .await
}

pub(crate) async fn delete_listed_local_branch_with_policy(
    workspace: &Workspace,
    requested: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git delete branch requires a Project root that equals the repository top level".into(),
        ));
    }
    if crate::repository_conflict::merge_in_progress(workspace).await?
        || crate::repository_conflict::rebase_in_progress(workspace).await?
        || crate::repository_conflict::cherry_pick_in_progress(workspace).await?
        || crate::repository_conflict::revert_in_progress(workspace).await?
    {
        return Err(AppError::InvalidRequest(
            "Git delete branch cannot run during an in-progress merge, rebase, cherry-pick, or revert"
                .into(),
        ));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "Git delete branch requires resolved conflicts".into(),
        ));
    }
    let listed = list_local_branches(workspace).await?;
    let status = repository::status(workspace).await?;
    let admitted = admit_listed_local_branch_delete(&listed, status.branch.as_deref(), requested)?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        &["branch", "-d", "--", admitted],
        MAX_GIT_DELETE_BRANCH_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository_branches::parse_local_branches;

    #[test]
    fn admits_a_listed_non_current_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(
            admit_listed_local_branch_delete(&listed, Some("main"), "topic").unwrap(),
            "topic"
        );
    }

    #[test]
    fn rejects_the_current_named_branch() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_delete(&listed, Some("main"), "main").is_err());
    }

    #[test]
    fn rejects_an_unlisted_or_truncated_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_delete(&listed, Some("main"), "other").is_err());
        let truncated = parse_local_branches(b"main\0top", true).unwrap();
        assert!(admit_listed_local_branch_delete(&truncated, Some("main"), "topic").is_err());
    }

    #[test]
    fn detached_head_may_delete_a_listed_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(
            admit_listed_local_branch_delete(&listed, Some("01234567 (detached)"), "topic")
                .unwrap(),
            "topic"
        );
    }

    fn fixture_repository() -> (std::path::PathBuf, Workspace) {
        use std::{fs, process::Command};
        use uuid::Uuid;

        let root = std::env::temp_dir().join(format!("sage-git-delete-branch-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "sage@example.invalid"],
            vec!["config", "user.name", "Sage Test"],
            vec!["config", "core.autocrlf", "false"],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(root.join("readme.txt"), "first\n").unwrap();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["add", "readme.txt"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["commit", "-q", "-m", "fixture"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["branch", "-m", "main"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["branch", "--", "topic"])
                .status()
                .unwrap()
                .success()
        );
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        (root, workspace)
    }

    #[test]
    fn deletes_a_listed_merged_name_without_switching() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(delete_listed_local_branch(&workspace, "topic"))
            .unwrap();
        let status = runtime.block_on(repository::status(&workspace)).unwrap();
        let listed = runtime.block_on(list_local_branches(&workspace)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(listed.names, ["main"]);
    }

    #[test]
    fn live_delete_rejects_the_current_named_branch() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(delete_listed_local_branch(&workspace, "main"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }

    #[test]
    fn live_delete_rejects_an_unlisted_name() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(delete_listed_local_branch(&workspace, "other"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }
}
