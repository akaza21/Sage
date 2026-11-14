use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::{
    domain::{
        GitFileMutationCommandError, GitRepositoryRebaseContinueRequest,
        GitRepositoryRebaseContinueResult,
    },
    repository_conflict,
    state::AppState,
    workspace::Workspace,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn continue_git_rebase(
    state: State<'_, AppState>,
    request: GitRepositoryRebaseContinueRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryRebaseContinueResult, GitFileMutationCommandError> {
    continue_git_rebase_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(rebase_command_error)
}

fn rebase_command_error(error: AppError) -> GitFileMutationCommandError {
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
            message: crate::repository_public_error::containment().into(),
        },
        error => GitFileMutationCommandError::Failed {
            message: crate::repository_public_error::rebase(&error),
        },
    }
}

async fn continue_git_rebase_inner(
    state: &AppState,
    request: GitRepositoryRebaseContinueRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitRepositoryRebaseContinueResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Continue rebase Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Continue rebase root identity exceeds its bounded contract".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested rebase continue".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Continue rebase does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Continue rebase root does not belong to the active Project".into(),
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
    let rebase_result = repository_conflict::continue_rebase(&workspace).await;
    let error = match rebase_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(crate::domain::GitFileMutationError {
            kind: crate::domain::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git rebase outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryRebaseContinueResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: crate::repository_index_status::project_status(repository_roots).await,
        error,
    })
}
