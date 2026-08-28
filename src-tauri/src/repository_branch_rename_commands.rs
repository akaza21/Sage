use tauri::State;

use crate::{
    domain::{
        GitFileMutationCommandError, GitRepositoryBranchRenameRequest, GitRepositoryCheckoutResult,
    },
    error::{AppError, AppResult, GitContainmentState},
    repository_branch_rename,
    state::AppState,
    workspace::Workspace,
};

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
                    crate::domain::GitProcessState::TerminatedUnreaped
                }
                GitContainmentState::PotentiallyRunning => {
                    crate::domain::GitProcessState::PotentiallyRunning
                }
            },
            message: crate::repository_public_error::containment().into(),
        },
        error => GitFileMutationCommandError::Failed {
            message: crate::repository_public_error::rename_branch(&error),
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
    let rename_result = repository_branch_rename::rename_listed_local_branch(
        &workspace,
        &request.from,
        &request.to,
    )
    .await;
    let error = match rename_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(crate::domain::GitFileMutationError {
            kind: crate::domain::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git rename branch outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
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
