use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::{repository_branch_create, state::AppState, workspace::Workspace};
use sage_domain::file_operations::{
    GitFileMutationCommandError, GitRepositoryCheckoutRequest, GitRepositoryCheckoutResult,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn create_git_local_branch(
    state: State<'_, AppState>,
    request: GitRepositoryCheckoutRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryCheckoutResult, GitFileMutationCommandError> {
    create_git_local_branch_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(create_branch_command_error)
}

fn create_branch_command_error(error: AppError) -> GitFileMutationCommandError {
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
            message: crate::repository_public_error::containment().into(),
        },
        error => GitFileMutationCommandError::Failed {
            message: crate::repository_public_error::create_branch(&error),
        },
    }
}

async fn create_git_local_branch_inner(
    state: &AppState,
    request: GitRepositoryCheckoutRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitRepositoryCheckoutResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git create branch Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git create branch root identity exceeds its bounded contract".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested Git create branch".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Git create branch does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Git create branch root does not belong to the active Project".into(),
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
    let create_result =
        repository_branch_create::create_unlisted_local_branch(&workspace, &request.branch).await;
    let error = match create_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(sage_domain::file_operations::GitFileMutationError {
            kind: sage_domain::file_operations::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git create branch outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryCheckoutResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: crate::repository_index_status::project_status(repository_roots).await,
        error,
    })
}
