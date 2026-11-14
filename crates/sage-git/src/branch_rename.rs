use sage_core::error::{AppError, AppResult};

use crate::{
    branches::{LocalBranchList, MAX_BRANCH_NAME_BYTES, list_local_branches, valid_branch_name},
    budget,
};
use sage_workspace::Workspace;

const MAX_GIT_RENAME_BRANCH_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) fn admit_listed_local_branch_rename<'a>(
    listed: &'a LocalBranchList,
    from: &'a str,
    to: &'a str,
) -> AppResult<(&'a str, &'a str)> {
    if from.is_empty()
        || from.len() > MAX_BRANCH_NAME_BYTES
        || !valid_branch_name(from)
        || to.is_empty()
        || to.len() > MAX_BRANCH_NAME_BYTES
        || !valid_branch_name(to)
    {
        return Err(AppError::InvalidRequest(
            "Git rename branch identity exceeds its bounded contract".into(),
        ));
    }
    if listed.truncated {
        return Err(AppError::InvalidRequest(
            "Git rename branch requires a complete local branch list".into(),
        ));
    }
    if from == to {
        return Err(AppError::InvalidRequest(
            "Git rename branch requires a different destination name".into(),
        ));
    }
    if listed.names.iter().any(|name| name == to) {
        return Err(AppError::InvalidRequest(
            "Git rename branch requires an unlisted destination name".into(),
        ));
    }
    if listed.names.iter().any(|name| name == from) {
        return Ok((from, to));
    }
    Err(AppError::InvalidRequest(
        "Git rename branch requires an exact listed local branch".into(),
    ))
}

pub async fn rename_listed_local_branch(
    workspace: &Workspace,
    from: &str,
    to: &str,
) -> AppResult<()> {
    rename_listed_local_branch_with_policy(
        workspace,
        from,
        to,
        crate::GitProcessPolicy::mutation(budget::MUTATION),
    )
    .await
}

pub(crate) async fn rename_listed_local_branch_with_policy(
    workspace: &Workspace,
    from: &str,
    to: &str,
    process_policy: crate::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::workspace::repository_boundary(workspace).await?,
        crate::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git rename branch requires a Project root that equals the repository top level".into(),
        ));
    }
    if crate::conflict::merge_in_progress(workspace).await?
        || crate::conflict::rebase_in_progress(workspace).await?
        || crate::conflict::cherry_pick_in_progress(workspace).await?
        || crate::conflict::revert_in_progress(workspace).await?
    {
        return Err(AppError::InvalidRequest(
            "Git rename branch cannot run during an in-progress merge, rebase, cherry-pick, or revert"
                .into(),
        ));
    }
    let unmerged =
        crate::workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1).await?;
    crate::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "Git rename branch requires resolved conflicts".into(),
        ));
    }
    let listed = list_local_branches(workspace).await?;
    let (admitted_from, admitted_to) = admit_listed_local_branch_rename(&listed, from, to)?;
    let output = crate::run_git_with_policy(
        workspace.root(),
        &["branch", "-m", "--", admitted_from, admitted_to],
        MAX_GIT_RENAME_BRANCH_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    crate::ensure_success(&output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branches::parse_local_branches;

    #[test]
    fn admits_a_listed_name_to_an_unlisted_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(
            admit_listed_local_branch_rename(&listed, "topic", "feature").unwrap(),
            ("topic", "feature")
        );
        assert_eq!(
            admit_listed_local_branch_rename(&listed, "main", "feature").unwrap(),
            ("main", "feature")
        );
    }

    #[test]
    fn rejects_a_listed_destination_or_same_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_rename(&listed, "topic", "main").is_err());
        assert!(admit_listed_local_branch_rename(&listed, "topic", "topic").is_err());
    }

    #[test]
    fn rejects_an_unlisted_or_truncated_source() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_rename(&listed, "other", "feature").is_err());
        let truncated = parse_local_branches(b"main\0top", true).unwrap();
        assert!(admit_listed_local_branch_rename(&truncated, "topic", "feature").is_err());
    }

    #[test]
    fn rejects_unsafe_destination_names() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_rename(&listed, "topic", "-bad").is_err());
        assert!(admit_listed_local_branch_rename(&listed, "topic", "has space").is_err());
        assert!(admit_listed_local_branch_rename(&listed, "topic", "a..b").is_err());
    }

    fn fixture_repository() -> (std::path::PathBuf, Workspace) {
        use std::fs;
        use uuid::Uuid;

        let root = std::env::temp_dir().join(format!("sage-git-rename-branch-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "sage@example.invalid"],
            vec!["config", "user.name", "Sage Test"],
            vec!["config", "core.autocrlf", "false"],
        ] {
            assert!(
                crate::process::fixture_git()
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
            crate::process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["add", "readme.txt"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["commit", "-q", "-m", "fixture"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["branch", "-m", "main"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::process::fixture_git()
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
    fn renames_a_listed_name_without_switching() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(rename_listed_local_branch(&workspace, "topic", "feature"))
            .unwrap();
        let status = runtime.block_on(crate::status(&workspace)).unwrap();
        let listed = runtime.block_on(list_local_branches(&workspace)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(listed.names, ["feature", "main"]);
    }

    #[test]
    fn live_rename_rejects_a_listed_destination() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(rename_listed_local_branch(&workspace, "topic", "main"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }

    #[test]
    fn live_rename_rejects_an_unlisted_source() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(rename_listed_local_branch(&workspace, "other", "feature"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }
}
