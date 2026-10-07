use crate::{
    error::{AppError, AppResult},
    git_budget, repository,
    repository_branches::{
        LocalBranchList, MAX_BRANCH_NAME_BYTES, MAX_LOCAL_BRANCHES, list_local_branches,
        valid_branch_name,
    },
    workspace::Workspace,
};

const MAX_GIT_CREATE_BRANCH_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) fn admit_unlisted_local_branch_create<'a>(
    listed: &LocalBranchList,
    requested: &'a str,
) -> AppResult<&'a str> {
    if requested.is_empty()
        || requested.len() > MAX_BRANCH_NAME_BYTES
        || !valid_branch_name(requested)
    {
        return Err(AppError::InvalidRequest(
            "Git create branch identity exceeds its bounded contract".into(),
        ));
    }
    if listed.truncated {
        return Err(AppError::InvalidRequest(
            "Git create branch requires a complete local branch list".into(),
        ));
    }
    if listed.names.len() >= MAX_LOCAL_BRANCHES {
        return Err(AppError::InvalidRequest(
            "Git create branch is capped at 64 local names".into(),
        ));
    }
    if listed.names.iter().any(|name| name == requested) {
        return Err(AppError::InvalidRequest(
            "Git create branch requires an unlisted local name".into(),
        ));
    }
    Ok(requested)
}

pub(crate) async fn create_unlisted_local_branch(
    workspace: &Workspace,
    requested: &str,
) -> AppResult<()> {
    create_unlisted_local_branch_with_policy(
        workspace,
        requested,
        repository::GitProcessPolicy::mutation(git_budget::MUTATION),
    )
    .await
}

pub(crate) async fn create_unlisted_local_branch_with_policy(
    workspace: &Workspace,
    requested: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git create branch requires a Project root that equals the repository top level".into(),
        ));
    }
    if crate::repository_conflict::merge_in_progress(workspace).await?
        || crate::repository_conflict::rebase_in_progress(workspace).await?
        || crate::repository_conflict::cherry_pick_in_progress(workspace).await?
        || crate::repository_conflict::revert_in_progress(workspace).await?
    {
        return Err(AppError::InvalidRequest(
            "Git create branch cannot run during an in-progress merge, rebase, cherry-pick, or revert"
                .into(),
        ));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "Git create branch requires resolved conflicts".into(),
        ));
    }
    let listed = list_local_branches(workspace).await?;
    let admitted = admit_unlisted_local_branch_create(&listed, requested)?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        &["branch", "--", admitted],
        MAX_GIT_CREATE_BRANCH_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository_branches::{LocalBranchList, parse_local_branches};

    #[test]
    fn admits_an_unlisted_valid_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(
            admit_unlisted_local_branch_create(&listed, "feature").unwrap(),
            "feature"
        );
    }

    #[test]
    fn rejects_a_listed_name() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_unlisted_local_branch_create(&listed, "main").is_err());
    }

    #[test]
    fn rejects_a_truncated_list() {
        let listed = parse_local_branches(b"main\0top", true).unwrap();
        assert!(admit_unlisted_local_branch_create(&listed, "feature").is_err());
    }

    #[test]
    fn rejects_unsafe_and_oversize_names() {
        let listed = parse_local_branches(b"main\0", false).unwrap();
        assert!(admit_unlisted_local_branch_create(&listed, "").is_err());
        assert!(admit_unlisted_local_branch_create(&listed, "-bad").is_err());
        assert!(admit_unlisted_local_branch_create(&listed, "has space").is_err());
        assert!(admit_unlisted_local_branch_create(&listed, "a..b").is_err());
        assert!(admit_unlisted_local_branch_create(&listed, &"m".repeat(256)).is_err());
    }

    #[test]
    fn rejects_a_full_64_name_list() {
        let listed = LocalBranchList {
            names: (0..64).map(|index| format!("b{index}")).collect(),
            truncated: false,
        };
        assert!(admit_unlisted_local_branch_create(&listed, "ok").is_err());
    }

    fn fixture_repository() -> (std::path::PathBuf, Workspace) {
        use std::{fs, process::Command};
        use uuid::Uuid;

        let root = std::env::temp_dir().join(format!("sage-git-create-branch-{}", Uuid::new_v4()));
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
                .args(["switch", "-q", "-c", "topic"])
                .status()
                .unwrap()
                .success()
        );
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        (root, workspace)
    }

    #[test]
    fn creates_an_unlisted_name_without_switching() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(create_unlisted_local_branch(&workspace, "feature"))
            .unwrap();
        let status = runtime.block_on(repository::status(&workspace)).unwrap();
        let listed = runtime.block_on(list_local_branches(&workspace)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(status.branch.as_deref(), Some("topic"));
        assert_eq!(listed.names, ["feature", "main", "topic"]);
    }

    #[test]
    fn live_create_rejects_a_listed_name() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(create_unlisted_local_branch(&workspace, "topic"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }
}
