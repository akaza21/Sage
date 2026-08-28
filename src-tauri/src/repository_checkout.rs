use std::time::Duration;

use crate::{
    error::{AppError, AppResult},
    repository,
    repository_branches::{
        LocalBranchList, MAX_BRANCH_NAME_BYTES, list_local_branches, named_branch,
    },
    workspace::Workspace,
};

const GIT_CHECKOUT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_GIT_CHECKOUT_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) fn admit_listed_local_branch_checkout<'a>(
    listed: &'a LocalBranchList,
    current: Option<&str>,
    requested: &str,
) -> AppResult<&'a str> {
    if requested.is_empty() || requested.len() > MAX_BRANCH_NAME_BYTES {
        return Err(AppError::InvalidRequest(
            "Git checkout branch identity exceeds its bounded contract".into(),
        ));
    }
    if named_branch(current) == Some(requested) {
        return Err(AppError::InvalidRequest(
            "Git checkout requires a different listed local branch".into(),
        ));
    }
    listed
        .names
        .iter()
        .find(|name| name.as_str() == requested)
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InvalidRequest("Git checkout requires an exact listed local branch".into())
        })
}

pub(crate) async fn checkout_listed_local_branch(
    workspace: &Workspace,
    requested: &str,
) -> AppResult<()> {
    checkout_listed_local_branch_with_policy(
        workspace,
        requested,
        repository::GitProcessPolicy::mutation(GIT_CHECKOUT_TIMEOUT),
    )
    .await
}

pub(crate) async fn checkout_listed_local_branch_with_policy(
    workspace: &Workspace,
    requested: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git checkout requires a Project root that equals the repository top level".into(),
        ));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "conflicted changes require the later conflict workflow".into(),
        ));
    }
    let listed = list_local_branches(workspace).await?;
    let status = repository::status(workspace).await?;
    let admitted =
        admit_listed_local_branch_checkout(&listed, status.branch.as_deref(), requested)?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        &["switch", "--no-guess", "-q", "--", admitted],
        MAX_GIT_CHECKOUT_OUTPUT_BYTES,
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
    fn admits_a_different_listed_local_branch() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(
            admit_listed_local_branch_checkout(&listed, Some("main"), "topic").unwrap(),
            "topic"
        );
    }

    #[test]
    fn rejects_the_current_named_branch() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_checkout(&listed, Some("main"), "main").is_err());
    }

    #[test]
    fn rejects_a_name_that_is_not_listed() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert!(admit_listed_local_branch_checkout(&listed, Some("main"), "other").is_err());
    }

    #[test]
    fn detached_head_may_switch_to_a_listed_branch() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(
            admit_listed_local_branch_checkout(&listed, Some("01234567 (detached)"), "main")
                .unwrap(),
            "main"
        );
    }

    #[test]
    fn rejects_empty_and_oversize_names() {
        let listed = parse_local_branches(b"main\0", false).unwrap();
        assert!(admit_listed_local_branch_checkout(&listed, None, "").is_err());
        assert!(admit_listed_local_branch_checkout(&listed, None, &"m".repeat(256)).is_err());
    }

    #[test]
    fn truncated_list_still_admits_an_exact_visible_name() {
        let listed = parse_local_branches(b"main\0top", true).unwrap();
        assert_eq!(
            admit_listed_local_branch_checkout(&listed, None, "main").unwrap(),
            "main"
        );
        assert!(admit_listed_local_branch_checkout(&listed, None, "top").is_err());
    }

    fn fixture_repository() -> (std::path::PathBuf, Workspace) {
        use std::{fs, process::Command};
        use uuid::Uuid;

        let root = std::env::temp_dir().join(format!("sage-git-checkout-{}", Uuid::new_v4()));
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
    fn switches_from_topic_to_the_listed_main_branch() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(checkout_listed_local_branch(&workspace, "main"))
            .unwrap();
        let status = runtime.block_on(repository::status(&workspace)).unwrap();
        let listed = runtime.block_on(list_local_branches(&workspace)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(listed.names, ["main", "topic"]);
    }

    #[test]
    fn live_checkout_rejects_the_current_named_branch() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(checkout_listed_local_branch(&workspace, "topic"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }

    #[test]
    fn live_checkout_rejects_an_unlisted_name() {
        let (root, workspace) = fixture_repository();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(checkout_listed_local_branch(&workspace, "other"));
        std::fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }
}
