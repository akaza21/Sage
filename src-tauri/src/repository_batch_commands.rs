use crate::project_registry::ProjectRegistryStore;
use crate::repository_mutation_commands::git_mutation_command_error;
use crate::state::AppState;
use sage_core::error::{AppError, AppResult};
use sage_domain::file_operations::{
    GitFileMutationAction, GitFileMutationCommandError, GitFileMutationError,
    GitFileMutationErrorKind, GitFileMutationResult, GitPathsMutationRequest,
};
use sage_workspace::Workspace;
use tauri::State;

#[tauri::command]
pub async fn stage_git_paths(
    state: State<'_, AppState>,
    request: GitPathsMutationRequest,
    expected_transition_epoch: u64,
) -> Result<GitFileMutationResult, GitFileMutationCommandError> {
    mutate_git_paths_inner(
        &state,
        request,
        GitFileMutationAction::Stage,
        expected_transition_epoch,
    )
    .await
    .map_err(git_mutation_command_error)
}

#[tauri::command]
pub async fn unstage_git_paths(
    state: State<'_, AppState>,
    request: GitPathsMutationRequest,
    expected_transition_epoch: u64,
) -> Result<GitFileMutationResult, GitFileMutationCommandError> {
    mutate_git_paths_inner(
        &state,
        request,
        GitFileMutationAction::Unstage,
        expected_transition_epoch,
    )
    .await
    .map_err(git_mutation_command_error)
}

#[tauri::command]
pub async fn discard_git_paths(
    state: State<'_, AppState>,
    request: GitPathsMutationRequest,
    expected_transition_epoch: u64,
) -> Result<GitFileMutationResult, GitFileMutationCommandError> {
    mutate_git_paths_inner(
        &state,
        request,
        GitFileMutationAction::Discard,
        expected_transition_epoch,
    )
    .await
    .map_err(git_mutation_command_error)
}

async fn mutate_git_paths_inner(
    state: &AppState,
    request: GitPathsMutationRequest,
    action: GitFileMutationAction,
    expected_transition_epoch: u64,
) -> AppResult<GitFileMutationResult> {
    if request.project_id.is_empty()
        || request.project_id.len() > 512
        || request.resources.is_empty()
        || request.resources.len() > sage_git::batch::MAX_BATCH_PATHS
    {
        return Err(AppError::InvalidRequest(
            "Git batch identity or path count exceeds its bounded contract".into(),
        ));
    }
    if action == GitFileMutationAction::Discard && !request.discard_confirmed {
        return Err(AppError::InvalidRequest(
            "Git discard requires explicit confirmation".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch
        || state.database.active_project_id()?.as_deref() != Some(&request.project_id)
    {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested Git batch".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let root_id = &request.resources[0].root_id;
    if request
        .resources
        .iter()
        .any(|resource| resource.root_id != *root_id)
    {
        return Err(AppError::InvalidRequest(
            "Git batch paths must belong to one Project root".into(),
        ));
    }
    let target = roots
        .iter()
        .find(|root| root.id == *root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest("Git batch root does not belong to the active Project".into())
        })?;
    let workspace =
        Workspace::open_registered_root(&target.project_id, &target.id, &target.absolute_path)?;
    let error = match sage_git::batch::mutate_paths(&workspace, &request.resources, action).await {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(message)) => Some(GitFileMutationError {
            kind: GitFileMutationErrorKind::EffectUnknown,
            message: format!(
                "{message}. Source Control was refreshed to show authoritative Git state."
            ),
        }),
        Err(error) => return Err(error),
    };
    let repository_roots = roots
        .into_iter()
        .map(|root| {
            let workspace =
                Workspace::open_registered_root(&root.project_id, &root.id, &root.absolute_path)
                    .map_err(|error| error.to_string());
            (root.id, workspace)
        })
        .collect();
    Ok(GitFileMutationResult {
        project_id: request.project_id,
        transition_epoch: *transition_epoch,
        repositories: sage_git::project_status(repository_roots).await,
        error,
    })
}

#[cfg(test)]
#[path = "repository_batch_commands_tests.rs"]
mod tests;
