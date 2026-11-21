use std::collections::BTreeSet;

use sage_core::error::{AppError, AppResult};
use sage_domain::{file_operations::GitFileMutationAction, resource::ResourceRef};
use sage_workspace::Workspace;

use crate::{
    GitProcessPolicy, budget,
    mutation::{mutation_paths, validate_resource_identity},
};

pub const MAX_BATCH_PATHS: usize = 300;

/// Validate the complete selection before issuing one contained Git mutation.
/// The caller owns the Project transition lock and the final status refresh.
pub async fn mutate_paths(
    workspace: &Workspace,
    resources: &[ResourceRef],
    action: GitFileMutationAction,
) -> AppResult<()> {
    if resources.is_empty() || resources.len() > MAX_BATCH_PATHS {
        return Err(AppError::InvalidRequest(
            "Git batch requires 1 to 300 paths".into(),
        ));
    }
    if !matches!(
        action,
        GitFileMutationAction::Stage
            | GitFileMutationAction::Unstage
            | GitFileMutationAction::Discard
    ) {
        return Err(AppError::InvalidRequest(
            "Git batch supports only stage, unstage and discard".into(),
        ));
    }
    for resource in resources {
        validate_resource_identity(workspace, resource)?;
    }
    let projected = crate::status(workspace).await?;
    if projected.state != "repository" || projected.truncated || projected.changes_overflowed {
        return Err(AppError::InvalidRequest(
            "Git batch requires complete repository status at its top level".into(),
        ));
    }
    let mut paths = BTreeSet::new();
    for resource in resources {
        let change = projected
            .changes
            .iter()
            .find(|change| change.resource == *resource)
            .ok_or_else(|| {
                AppError::InvalidRequest("resource is not a current Git change".into())
            })?;
        for path in mutation_paths(workspace, change, action)? {
            if paths.len() == MAX_BATCH_PATHS && !paths.contains(&path) {
                return Err(AppError::InvalidRequest(
                    "Git batch exceeds 300 paths including rename sources".into(),
                ));
            }
            paths.insert(path);
        }
    }
    let mut arguments = match action {
        GitFileMutationAction::Stage => vec!["add"],
        // Path-limited reset also handles an unborn HEAD, without a separate probe.
        GitFileMutationAction::Unstage => vec!["reset", "--quiet"],
        GitFileMutationAction::Discard => vec!["restore", "--worktree"],
        _ => unreachable!("batch action was validated"),
    };
    arguments.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    // The contained runner supplies --literal-pathspecs. Keep the bounded paths
    // off the command line, which has a 32,767-character limit on Windows.
    let input = paths.into_iter().fold(Vec::new(), |mut input, path| {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
        input
    });
    let output = crate::cli::run_git_with_policy(
        workspace.root(),
        &arguments,
        64 * 1024,
        GitProcessPolicy::mutation(budget::MUTATION).with_stdin(input),
    )
    .await?;
    // A multi-path restore may change earlier paths before a later path fails.
    // Always refresh authoritative status after a nonzero mutation exit.
    crate::ensure_success(&output).map_err(|error| AppError::GitEffectUnknown(error.to_string()))
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
