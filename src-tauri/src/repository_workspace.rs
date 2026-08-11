use crate::{
    error::{AppError, AppResult},
    repository::{self, GitOutput, GitProcessPolicy},
    workspace::Workspace,
};

pub(crate) async fn run_git(
    workspace: &Workspace,
    args: &[&str],
    stdout_limit: usize,
) -> AppResult<GitOutput> {
    workspace.ensure_root_identity()?;
    let result = repository::run_git(workspace.root(), args, stdout_limit).await;
    reconcile_identity(result, workspace.ensure_root_identity(), false)
}

pub(crate) async fn run_git_with_policy(
    workspace: &Workspace,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    run_git_with_policy_inner(workspace, args, stdout_limit, policy, || {}).await
}

async fn run_git_with_policy_inner<F>(
    workspace: &Workspace,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
    after_process: F,
) -> AppResult<GitOutput>
where
    F: FnOnce(),
{
    workspace.ensure_root_identity()?;
    let visible_effect = policy.has_visible_effect();
    let result =
        repository::run_git_with_policy(workspace.root(), args, stdout_limit, policy).await;
    after_process();
    reconcile_identity(result, workspace.ensure_root_identity(), visible_effect)
}

fn reconcile_identity(
    result: AppResult<GitOutput>,
    identity: AppResult<()>,
    visible_effect: bool,
) -> AppResult<GitOutput> {
    if matches!(&result, Err(AppError::GitContainment { .. })) || identity.is_ok() {
        return result;
    }
    let identity = identity.unwrap_err();
    if visible_effect {
        Err(AppError::GitEffectUnknown(format!(
            "{identity}; root identity changed after a visible Git mutation"
        )))
    } else {
        Err(identity)
    }
}

#[cfg(test)]
#[path = "repository_workspace_tests.rs"]
mod tests;
