#[cfg(test)]
use std::time::Duration;

use tauri::State;

use crate::{
    domain::{GitFileMutationCommandError, GitRepositoryCommitRequest, GitRepositoryCommitResult},
    error::{AppError, AppResult, GitContainmentState},
    repository, repository_commit,
    state::AppState,
    workspace::Workspace,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;
const MAX_PUBLIC_GIT_ERROR_BYTES: usize = 512;

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
                    crate::domain::GitProcessState::TerminatedUnreaped
                }
                GitContainmentState::PotentiallyRunning => {
                    crate::domain::GitProcessState::PotentiallyRunning
                }
            },
            message: "A prior Git process has not finished exiting. Wait for Source Control recovery before retrying."
                .into(),
        },
        error => GitFileMutationCommandError::Failed {
            message: public_commit_error(&error),
        },
    }
}

fn public_commit_error(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not complete the commit. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this Git commit.",
        AppError::GitEffectUnknown(_) => {
            "The Git commit outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => {
            "A prior Git process has not finished exiting. Wait for Source Control recovery before retrying."
        }
        AppError::UnknownHarness(_) | AppError::HarnessUnavailable(_) => {
            "Sage could not complete the Git commit."
        }
    };
    truncate_public_error(message)
}

fn truncate_public_error(message: &str) -> String {
    if message.len() <= MAX_PUBLIC_GIT_ERROR_BYTES {
        return message.into();
    }
    let mut boundary = MAX_PUBLIC_GIT_ERROR_BYTES;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message[..boundary].into()
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
        Some(repository::GitProcessPolicy::mutation(mutation_timeout)),
    )
    .await
}

async fn commit_git_repository_inner_with_policy_override(
    state: &AppState,
    request: GitRepositoryCommitRequest,
    expected_transition_epoch: u64,
    process_policy: Option<repository::GitProcessPolicy>,
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
    let message = repository_commit::normalize_message(&request.message)?;
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
            }
            .map_err(|error| error.to_string());
            (root.id.clone(), workspace)
        })
        .collect();
    let commit_result = match process_policy {
        Some(policy) => {
            repository_commit::commit_staged_with_policy(&workspace, &message, policy).await
        }
        None => repository_commit::commit_staged(&workspace, &message).await,
    };
    let error = match commit_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(crate::domain::GitFileMutationError {
            kind: crate::domain::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git commit outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryCommitResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: crate::repository_index_status::project_status(repository_roots).await,
        error,
    })
}

#[cfg(test)]
#[path = "repository_commit_commands_tests.rs"]
mod tests;
