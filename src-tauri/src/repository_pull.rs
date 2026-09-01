use std::time::Duration;

use crate::{
    error::{AppError, AppResult},
    repository, repository_branches, repository_conflict,
    workspace::Workspace,
};

const GIT_PULL_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_GIT_PULL_OUTPUT_BYTES: usize = 64 * 1024;

/// Integrates only a known-behind branch through Git's fast-forward-only mode.
pub(crate) async fn pull_fast_forward(workspace: &Workspace) -> AppResult<()> {
    pull_fast_forward_with_policy(
        workspace,
        repository::GitProcessPolicy::mutation(GIT_PULL_TIMEOUT),
    )
    .await
}

pub(crate) async fn pull_fast_forward_with_policy(
    workspace: &Workspace,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    admit_fast_forward_pull(workspace).await?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        &["pull", "-q", "--ff-only"],
        MAX_GIT_PULL_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)
}

async fn admit_fast_forward_pull(workspace: &Workspace) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git pull requires a Project root that equals the repository top level".into(),
        ));
    }
    if repository_conflict::merge_in_progress(workspace).await?
        || repository_conflict::rebase_in_progress(workspace).await?
        || repository_conflict::cherry_pick_in_progress(workspace).await?
        || repository_conflict::revert_in_progress(workspace).await?
    {
        return Err(AppError::InvalidRequest(
            "Git pull cannot run during an in-progress merge, rebase, cherry-pick, or revert"
                .into(),
        ));
    }

    let status = repository::status(workspace).await?;
    if status.truncated || !status.changes.is_empty() {
        return Err(AppError::InvalidRequest(
            "Git pull requires no local changes and a complete repository projection".into(),
        ));
    }
    if repository_branches::named_branch(status.branch.as_deref()).is_none()
        || status.upstream.is_none()
    {
        return Err(AppError::InvalidRequest(
            "Git pull requires a current named branch with an upstream".into(),
        ));
    }
    if status.behind == 0 || status.ahead != 0 {
        return Err(AppError::InvalidRequest(
            "Git pull requires a branch that is behind its upstream without local divergence"
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use uuid::Uuid;

    use super::*;

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

    fn behind_clone() -> (std::path::PathBuf, std::path::PathBuf, Workspace) {
        let stamp = Uuid::new_v4();
        let bare = std::env::temp_dir().join(format!("sage-git-pull-bare-{stamp}"));
        let source = std::env::temp_dir().join(format!("sage-git-pull-source-{stamp}"));
        let dest = std::env::temp_dir().join(format!("sage-git-pull-dest-{stamp}"));
        fs::create_dir_all(&bare).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();
        git(&bare, &["init", "--bare", "-q", "-b", "main"]);
        git(&source, &["init", "-q", "-b", "main"]);
        git(&source, &["config", "user.email", "sage@example.invalid"]);
        git(&source, &["config", "user.name", "Sage Test"]);
        fs::write(source.join("tracked.txt"), "one\n").unwrap();
        git(&source, &["add", "--", "tracked.txt"]);
        git(&source, &["commit", "-q", "-m", "initial"]);
        git(&source, &["remote", "add", "origin", &remote_path(&bare)]);
        git(&source, &["push", "-q", "-u", "origin", "main"]);
        git(&dest, &["clone", "-q", &remote_path(&bare), "."]);
        fs::write(source.join("tracked.txt"), "two\n").unwrap();
        git(&source, &["add", "--", "tracked.txt"]);
        git(&source, &["commit", "-q", "-m", "incoming"]);
        git(&source, &["push", "-q", "origin", "main"]);
        git(&dest, &["fetch", "-q", "origin"]);
        let workspace = Workspace::open("project-a", "root-a", &dest).unwrap();
        (bare, source, workspace)
    }

    fn cleanup(bare: std::path::PathBuf, source: std::path::PathBuf, workspace: &Workspace) {
        fs::remove_dir_all(bare).unwrap();
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(workspace.root()).unwrap();
    }

    #[test]
    fn pulls_a_clean_behind_branch_with_fast_forward_only() {
        let (bare, source, workspace) = behind_clone();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(pull_fast_forward(&workspace))
            .unwrap();
        assert_eq!(
            git_stdout(workspace.root(), &["rev-parse", "HEAD"]).trim(),
            git_stdout(&bare, &["rev-parse", "refs/heads/main"]).trim()
        );
        cleanup(bare, source, &workspace);
    }

    #[test]
    fn refuses_diverged_or_dirty_history() {
        let (bare, source, workspace) = behind_clone();
        git(
            workspace.root(),
            &["config", "user.email", "sage@example.invalid"],
        );
        git(workspace.root(), &["config", "user.name", "Sage Test"]);
        fs::write(workspace.root().join("local.txt"), "local\n").unwrap();
        git(workspace.root(), &["add", "--", "local.txt"]);
        git(workspace.root(), &["commit", "-q", "-m", "local"]);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let diverged = runtime.block_on(pull_fast_forward(&workspace)).unwrap_err();
        assert!(diverged.to_string().contains("without local divergence"));
        git(workspace.root(), &["reset", "--hard", "-q", "origin/main"]);
        fs::write(workspace.root().join("dirty.txt"), "dirty\n").unwrap();
        let dirty = runtime.block_on(pull_fast_forward(&workspace)).unwrap_err();
        assert!(dirty.to_string().contains("no local changes"));
        cleanup(bare, source, &workspace);
    }
}
