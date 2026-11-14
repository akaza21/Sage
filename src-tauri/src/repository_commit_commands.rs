#[cfg(test)]
use std::time::Duration;

use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;
use sage_domain::file_operations::{
    GitFileMutationCommandError, GitRepositoryCommitRequest, GitRepositoryCommitResult,
};
use sage_workspace::Workspace;

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn commit_git_repository(
    state: State<'_, AppState>,
    request: GitRepositoryCommitRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryCommitResult, GitFileMutationCommandError> {
    commit_git_repository_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(commit_command_error)
}

fn commit_command_error(error: AppError) -> GitFileMutationCommandError {
    match error {
        AppError::GitContainment { state, .. } => GitFileMutationCommandError::Containment {
            process_state: match state {
                GitContainmentState::TerminatedUnreaped => {
                    sage_domain::file_operations::GitProcessState::TerminatedUnreaped
                }
                GitContainmentState::PotentiallyRunning => {
                    sage_domain::file_operations::GitProcessState::PotentiallyRunning
                }
            },
            message: sage_git::public_error::containment().into(),
        },
        error => GitFileMutationCommandError::Failed {
            message: sage_git::public_error::commit(&error),
        },
    }
}

async fn commit_git_repository_inner(
    state: &AppState,
    request: GitRepositoryCommitRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitRepositoryCommitResult> {
    commit_git_repository_inner_with_policy_override(
        state,
        request,
        expected_transition_epoch,
        None,
    )
    .await
}

#[cfg(test)]
async fn commit_git_repository_inner_with_timeout(
    state: &AppState,
    request: GitRepositoryCommitRequest,
    expected_transition_epoch: u64,
    mutation_timeout: Duration,
) -> AppResult<GitRepositoryCommitResult> {
    commit_git_repository_inner_with_policy_override(
        state,
        request,
        expected_transition_epoch,
        Some(sage_git::process::GitProcessPolicy::post_spawn_failure(
            mutation_timeout,
        )),
    )
    .await
}

async fn commit_git_repository_inner_with_policy_override(
    state: &AppState,
    request: GitRepositoryCommitRequest,
    expected_transition_epoch: u64,
    process_policy: Option<sage_git::process::GitProcessPolicy>,
) -> AppResult<GitRepositoryCommitResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git commit Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git commit root identity exceeds its bounded contract".into(),
        ));
    }
    let message = sage_git::commit::normalize_message(&request.message)?;
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested Git commit".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Git commit does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest("Git commit root does not belong to the active Project".into())
        })?;
    let workspace = Workspace::open(&target.project_id, &target.id, &target.absolute_path)?;
    let repository_roots = roots
        .iter()
        .map(|root| {
            let workspace = if root.id == target.id {
                Ok(workspace.clone())
            } else {
                Workspace::open(&root.project_id, &root.id, &root.absolute_path)
            };
            (root.id.clone(), workspace)
        })
        .collect();
    let commit_result = match process_policy {
        Some(policy) => {
            sage_git::commit::commit_staged_with_policy(&workspace, &message, policy).await
        }
        None => sage_git::commit::commit_staged(&workspace, &message).await,
    };
    let error = match commit_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(sage_domain::file_operations::GitFileMutationError {
            kind: sage_domain::file_operations::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git commit outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryCommitResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: sage_git::index_status::index_project_status(repository_roots).await,
        error,
    })
}

#[cfg(test)]
#[path = "repository_commit_commands_tests.rs"]
mod tests;
