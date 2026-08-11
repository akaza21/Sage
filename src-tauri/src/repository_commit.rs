use std::time::Duration;

use crate::{
    error::{AppError, AppResult},
    repository,
    workspace::Workspace,
};

pub(crate) const MAX_GIT_COMMIT_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_GIT_IDENTITY_BYTES: usize = 512;
const GIT_COMMIT_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) async fn commit_staged(workspace: &Workspace, message: &str) -> AppResult<()> {
    commit_staged_with_policy(
        workspace,
        message,
        repository::GitProcessPolicy::mutation(GIT_COMMIT_TIMEOUT),
    )
    .await
}

#[cfg(test)]
pub(crate) async fn commit_staged_with_timeout(
    workspace: &Workspace,
    message: &str,
    mutation_timeout: Duration,
) -> AppResult<()> {
    commit_staged_with_policy(
        workspace,
        message,
        repository::GitProcessPolicy::mutation(mutation_timeout),
    )
    .await
}

pub(crate) async fn commit_staged_with_policy(
    workspace: &Workspace,
    message: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    let message = normalize_message(message)?;
    let status = repository::status(workspace).await?;
    if status.state != "repository" {
        return Err(AppError::InvalidRequest(
            "Git commit requires a Project root that equals the repository top level".into(),
        ));
    }
    if status.truncated {
        return Err(AppError::ResourceLimit(
            "Git commit requires a complete Source Control projection".into(),
        ));
    }
    if status
        .branch
        .as_deref()
        .is_none_or(|branch| branch.ends_with(" (detached)"))
    {
        return Err(AppError::InvalidRequest(
            "detached HEAD requires the later Git workflow".into(),
        ));
    }
    reject_unfinished_history(workspace).await?;
    if status
        .changes
        .iter()
        .any(|change| change.index_status == "conflicted" || change.worktree_status == "conflicted")
    {
        return Err(AppError::InvalidRequest(
            "conflicted changes require the later conflict workflow".into(),
        ));
    }
    if !status
        .changes
        .iter()
        .any(|change| change.index_status != "unchanged" && change.index_status != "unknown")
    {
        return Err(AppError::InvalidRequest(
            "Git commit requires at least one staged change".into(),
        ));
    }

    let name = local_identity_value(workspace, "user.name").await?;
    let email = local_identity_value(workspace, "user.email").await?;
    let name_config = format!("user.name={name}");
    let email_config = format!("user.email={email}");
    let arguments = [
        "-c",
        name_config.as_str(),
        "-c",
        email_config.as_str(),
        "-c",
        "user.useConfigOnly=true",
        "commit",
        "--quiet",
        "--no-verify",
        "--no-gpg-sign",
        "--no-status",
        "--cleanup=verbatim",
        "-m",
        message.as_str(),
    ];
    let output =
        repository::run_git_with_policy(workspace.root(), &arguments, 64 * 1024, process_policy)
            .await?;
    repository::ensure_success(&output)
}

pub(crate) fn normalize_message(message: &str) -> AppResult<String> {
    let normalized = message.trim();
    if normalized.is_empty()
        || normalized.len() > MAX_GIT_COMMIT_MESSAGE_BYTES
        || normalized.contains('\0')
    {
        return Err(AppError::InvalidRequest(format!(
            "Git commit message must contain 1 to {MAX_GIT_COMMIT_MESSAGE_BYTES} UTF-8 bytes and no NUL"
        )));
    }
    Ok(normalized.into())
}

async fn local_identity_value(workspace: &Workspace, key: &str) -> AppResult<String> {
    let output = repository::run_git(
        workspace.root(),
        &["config", "--local", "--no-includes", "--get", key],
        MAX_GIT_IDENTITY_BYTES + 2,
    )
    .await?;
    if output.exit_code() == Some(1) {
        return Err(AppError::InvalidRequest(format!(
            "Git commit requires repository-local {key}"
        )));
    }
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::InvalidRequest(format!(
            "repository-local {key} exceeds its bounded contract"
        )));
    }
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::InvalidRequest(format!("repository-local {key} is not UTF-8")))?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty()
        || value.len() > MAX_GIT_IDENTITY_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "repository-local {key} is not a valid bounded identity"
        )));
    }
    Ok(value.into())
}

async fn reject_unfinished_history(workspace: &Workspace) -> AppResult<()> {
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
    ] {
        let output = repository::run_git(
            workspace.root(),
            &["rev-parse", "--verify", "--quiet", marker],
            256,
        )
        .await?;
        if output.succeeded() {
            return Err(AppError::InvalidRequest(
                "unfinished merge, cherry-pick, revert, or rebase requires the later Git workflow"
                    .into(),
            ));
        }
        if output.exit_code() != Some(1) {
            repository::ensure_success(&output)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "repository_commit_tests.rs"]
mod tests;
