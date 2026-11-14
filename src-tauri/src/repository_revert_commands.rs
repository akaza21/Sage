use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;
use sage_domain::file_operations::{
    GitFileMutationCommandError, GitRepositoryRevertContinueRequest,
    GitRepositoryRevertContinueResult,
};
use sage_workspace::Workspace;

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn continue_git_revert(
    state: State<'_, AppState>,
    request: GitRepositoryRevertContinueRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryRevertContinueResult, GitFileMutationCommandError> {
    continue_git_revert_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(revert_command_error)
}

fn revert_command_error(error: AppError) -> GitFileMutationCommandError {
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
            message: sage_git::public_error::revert(&error),
        },
    }
}

async fn continue_git_revert_inner(
    state: &AppState,
    request: GitRepositoryRevertContinueRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitRepositoryRevertContinueResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Continue revert Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Continue revert root identity exceeds its bounded contract".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested revert continue".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Continue revert does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Continue revert root does not belong to the active Project".into(),
            )
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
    let revert_result = sage_git::revert::continue_revert(&workspace).await;
    let error = match revert_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(sage_domain::file_operations::GitFileMutationError {
            kind: sage_domain::file_operations::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git revert outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryRevertContinueResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: sage_git::index_status::index_project_status(repository_roots).await,
        error,
    })
}
