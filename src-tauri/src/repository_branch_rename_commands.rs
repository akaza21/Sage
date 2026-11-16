use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;
use sage_domain::file_operations::{
    GitFileMutationCommandError, GitRepositoryBranchRenameRequest, GitRepositoryCheckoutResult,
};
use sage_workspace::Workspace;

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn rename_git_local_branch(
    state: State<'_, AppState>,
    request: GitRepositoryBranchRenameRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryCheckoutResult, GitFileMutationCommandError> {
    rename_git_local_branch_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(rename_branch_command_error)
}

fn rename_branch_command_error(error: AppError) -> GitFileMutationCommandError {
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
            message: sage_git::public_error::rename_branch(&error),
        },
    }
}

async fn rename_git_local_branch_inner(
    state: &AppState,
    request: GitRepositoryBranchRenameRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitRepositoryCheckoutResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git rename branch Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git rename branch root identity exceeds its bounded contract".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested Git rename branch".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Git rename branch does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Git rename branch root does not belong to the active Project".into(),
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
    let rename_result =
        sage_git::branch_rename::rename_listed_local_branch(&workspace, &request.from, &request.to)
            .await;
    let error = match rename_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(sage_domain::file_operations::GitFileMutationError {
            kind: sage_domain::file_operations::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git rename branch outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryCheckoutResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: sage_git::index_status::index_project_status(repository_roots).await,
        error,
    })
}
