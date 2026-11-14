use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::{
    domain::{GitFileMutationCommandError, GitRepositoryPullRequest, GitRepositoryPullResult},
    repository_pull,
    state::AppState,
    workspace::Workspace,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn pull_git_repository(
    state: State<'_, AppState>,
    request: GitRepositoryPullRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryPullResult, GitFileMutationCommandError> {
    pull_git_repository_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(pull_command_error)
}

fn pull_command_error(error: AppError) -> GitFileMutationCommandError {
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
            message: crate::repository_public_error::pull(&error),
        },
    }
}

async fn pull_git_repository_inner(
    state: &AppState,
    request: GitRepositoryPullRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitRepositoryPullResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git pull Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git pull root identity exceeds its bounded contract".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested Git pull".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Git pull does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest("Git pull root does not belong to the active Project".into())
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
    let pull_result = repository_pull::pull_fast_forward(&workspace).await;
    let error = match pull_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(crate::domain::GitFileMutationError {
            kind: crate::domain::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git pull outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryPullResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: crate::repository_index_status::project_status(repository_roots).await,
        error,
    })
}
