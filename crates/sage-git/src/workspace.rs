use sage_core::error::{AppError, AppResult, RepositoryLayout};

use crate::{GitOutput, GitProcessPolicy, budget};
use sage_workspace::Workspace;

pub(crate) async fn repository_boundary(
    workspace: &Workspace,
) -> AppResult<crate::RepositoryBoundary> {
    // Admission refuses a root below the top level before Git runs; that is
    // the nested boundary, not a failure.
    let output = match run_git(workspace, &["rev-parse", "--show-toplevel"], 32 * 1024).await {
        Err(AppError::RepositoryOutsideRoot(RepositoryLayout::NestedRoot)) => {
            workspace.ensure_root_identity()?;
            return Ok(crate::RepositoryBoundary::Nested);
        }
        output => output?,
    };
    let boundary = crate::repository_boundary_from_output(workspace.root(), &output)?;
    workspace.ensure_root_identity()?;
    Ok(boundary)
}

pub(crate) async fn run_git(
    workspace: &Workspace,
    args: &[&str],
    stdout_limit: usize,
) -> AppResult<GitOutput> {
    let authority = crate::process::GitRootAuthority::acquire(workspace)?;
    let result = crate::run_git_with_authority(
        workspace.root(),
        args,
        stdout_limit,
        GitProcessPolicy::read(budget::READ),
        authority,
    )
    .await;
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
    let authority = crate::process::GitRootAuthority::acquire(workspace)?;
    let visible_effect = policy.has_visible_effect();
    #[cfg(any(test, feature = "test-support"))]
    let policy_after_process = policy.after_process_hook();
    let result =
        crate::run_git_with_authority(workspace.root(), args, stdout_limit, policy, authority)
            .await;
    #[cfg(any(test, feature = "test-support"))]
    if let Some(hook) = policy_after_process {
        hook();
    }
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
mod tests;
