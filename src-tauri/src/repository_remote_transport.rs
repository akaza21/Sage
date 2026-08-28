use std::time::Duration;

use crate::{
    error::{AppError, AppResult},
    repository, repository_branches, repository_conflict, repository_remotes,
    workspace::Workspace,
};

const GIT_REMOTE_TRANSPORT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_GIT_MUTATION_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) async fn fetch_listed_remote(workspace: &Workspace, requested: &str) -> AppResult<()> {
    fetch_listed_remote_with_policy(
        workspace,
        requested,
        repository::GitProcessPolicy::mutation(GIT_REMOTE_TRANSPORT_TIMEOUT),
    )
    .await
}

pub(crate) async fn push_listed_remote(workspace: &Workspace, requested: &str) -> AppResult<()> {
    push_listed_remote_with_policy(
        workspace,
        requested,
        repository::GitProcessPolicy::mutation(GIT_REMOTE_TRANSPORT_TIMEOUT),
    )
    .await
}

pub(crate) async fn fetch_listed_remote_with_policy(
    workspace: &Workspace,
    requested: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    let admitted = admit_idle_listed_remote(workspace, requested, "Git fetch").await?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        &["fetch", "-q", "--no-tags", "--", &admitted],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)
}

pub(crate) async fn push_listed_remote_with_policy(
    workspace: &Workspace,
    requested: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    let admitted = admit_idle_listed_remote(workspace, requested, "Git push").await?;
    let branch = current_listed_named_branch(workspace, "Git push").await?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        &["push", "-q", "--", &admitted, branch.as_str()],
        MAX_GIT_MUTATION_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)
}

async fn admit_idle_listed_remote(
    workspace: &Workspace,
    requested: &str,
    action: &str,
) -> AppResult<String> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(format!(
            "{action} requires a Project root that equals the repository top level"
        )));
    }
    if repository_conflict::merge_in_progress(workspace).await?
        || repository_conflict::rebase_in_progress(workspace).await?
        || repository_conflict::cherry_pick_in_progress(workspace).await?
        || repository_conflict::revert_in_progress(workspace).await?
    {
        return Err(AppError::InvalidRequest(format!(
            "{action} cannot run during an in-progress merge, rebase, cherry-pick, or revert"
        )));
    }
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(format!(
            "{action} requires resolved conflicts"
        )));
    }
    let listed = repository_remotes::list_remotes(workspace).await?;
    Ok(repository_remotes::admit_listed_remote(&listed, requested)?.to_owned())
}

async fn current_listed_named_branch(workspace: &Workspace, action: &str) -> AppResult<String> {
    let status = repository::status(workspace).await?;
    let current = repository_branches::named_branch(status.branch.as_deref()).ok_or_else(|| {
        AppError::InvalidRequest(format!("{action} requires a current named branch"))
    })?;
    let listed = repository_branches::list_local_branches(workspace).await?;
    listed
        .names
        .iter()
        .find(|name| name.as_str() == current)
        .cloned()
        .ok_or_else(|| {
            AppError::InvalidRequest(format!("{action} requires a current listed local branch"))
        })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use uuid::Uuid;

    use super::*;
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

    fn git_stdout(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    fn remote_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn local_remote_pair() -> (std::path::PathBuf, std::path::PathBuf, Workspace) {
        let stamp = Uuid::new_v4();
        let bare = std::env::temp_dir().join(format!("sage-git-bare-{stamp}"));
        let work = std::env::temp_dir().join(format!("sage-git-work-{stamp}"));
        fs::create_dir_all(&bare).unwrap();
        fs::create_dir_all(&work).unwrap();
        git(&bare, &["init", "--bare", "-q", "-b", "main"]);
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["config", "user.email", "sage@example.invalid"]);
        git(&work, &["config", "user.name", "Sage Test"]);
        fs::write(work.join("ok.txt"), "ok\n").unwrap();
        git(&work, &["add", "--", "ok.txt"]);
        git(&work, &["commit", "-q", "-m", "ok"]);
        git(&work, &["remote", "add", "origin", &remote_path(&bare)]);
        let workspace = Workspace::open("project-a", "root-a", &work).unwrap();
        (bare, work, workspace)
    }

    #[test]
    fn push_listed_remote_updates_the_bare_named_branch() {
        let (bare, work, workspace) = local_remote_pair();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(push_listed_remote(&workspace, "origin"))
            .unwrap();
        assert_eq!(
            git_stdout(&work, &["rev-parse", "HEAD"]).trim(),
            git_stdout(&bare, &["rev-parse", "refs/heads/main"]).trim()
        );
        fs::remove_dir_all(bare).unwrap();
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn fetch_listed_remote_updates_remote_tracking_without_urls() {
        let (bare, source, source_workspace) = local_remote_pair();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(push_listed_remote(&source_workspace, "origin"))
            .unwrap();
        let dest = std::env::temp_dir().join(format!("sage-git-fetch-{}", Uuid::new_v4()));
        fs::create_dir_all(&dest).unwrap();
        git(&dest, &["init", "-q", "-b", "main"]);
        git(&dest, &["remote", "add", "origin", &remote_path(&bare)]);
        let dest_workspace = Workspace::open("project-a", "root-a", &dest).unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(fetch_listed_remote(&dest_workspace, "origin"))
            .unwrap();
        assert_eq!(
            git_stdout(&bare, &["rev-parse", "refs/heads/main"]).trim(),
            git_stdout(&dest, &["rev-parse", "refs/remotes/origin/main"]).trim()
        );
        fs::remove_dir_all(bare).unwrap();
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(dest).unwrap();
    }

    #[test]
    fn fetch_rejects_unlisted_remotes_and_urls() {
        let (bare, work, workspace) = local_remote_pair();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let missing = runtime
            .block_on(fetch_listed_remote(&workspace, "upstream"))
            .unwrap_err();
        assert!(missing.to_string().contains("exact listed remote name"));
        assert!(!missing.to_string().contains("http"));
        let url = runtime
            .block_on(fetch_listed_remote(
                &workspace,
                "https://example.invalid/repo.git",
            ))
            .unwrap_err();
        assert!(url.to_string().contains("exact listed remote name"));
        assert!(!url.to_string().contains("example.invalid"));
        fs::remove_dir_all(bare).unwrap();
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn fetch_rejects_an_in_progress_merge() {
        let (bare, work, workspace) = local_remote_pair();
        let oid = git_stdout(&work, &["rev-parse", "HEAD"]);
        fs::write(work.join(".git").join("MERGE_HEAD"), oid).unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(fetch_listed_remote(&workspace, "origin"))
            .unwrap_err();
        assert!(error.to_string().contains("in-progress merge"));
        fs::remove_dir_all(bare).unwrap();
        fs::remove_dir_all(work).unwrap();
    }

    #[test]
    fn push_rejects_detached_head() {
        let (bare, work, workspace) = local_remote_pair();
        git(&work, &["checkout", "--detach", "-q"]);
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(push_listed_remote(&workspace, "origin"))
            .unwrap_err();
        assert!(error.to_string().contains("named branch"));
        fs::remove_dir_all(bare).unwrap();
        fs::remove_dir_all(work).unwrap();
    }
}
