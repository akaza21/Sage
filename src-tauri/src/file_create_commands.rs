use tauri::State;

use crate::{
    domain::{CreateProjectFileRequest, CreateProjectFileResult},
    error::{AppError, AppResult},
    state::AppState,
    workspace::Workspace,
};

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[tauri::command]
pub async fn create_project_file(
    state: State<'_, AppState>,
    request: CreateProjectFileRequest,
) -> Result<CreateProjectFileResult, String> {
    execute(&state, request)
        .await
        .map_err(|error| error.to_string())
}

async fn execute(
    state: &AppState,
    request: CreateProjectFileRequest,
) -> AppResult<CreateProjectFileResult> {
    if request.project_generation > MAX_JAVASCRIPT_SAFE_INTEGER
        || request.expected_transition_epoch > MAX_JAVASCRIPT_SAFE_INTEGER
    {
        return Err(AppError::InvalidRequest(
            "file creation intent metadata is outside the safe integer range".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != request.expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "project changed before file creation could run".into(),
        ));
    }
    let database = state.database.clone();
    let response_epoch = *transition_epoch;
    tauri::async_runtime::spawn_blocking(move || {
        if database.active_project_id()?.as_deref() != Some(&request.project_id) {
            return Err(AppError::InvalidRequest(
                "file creation Project is not active".into(),
            ));
        }
        let root = database.root_record(&request.parent.root_id)?;
        if root.project_id != request.project_id {
            return Err(AppError::InvalidRequest(
                "file creation root does not belong to the active Project".into(),
            ));
        }
        let workspace = Workspace::open(&root.project_id, &root.id, &root.absolute_path)?;
        let resource = workspace.create_empty_file(&request.parent, &request.basename)?;
        let parent_listing = workspace.list_directory(&request.parent)?;
        Ok(CreateProjectFileResult {
            project_id: request.project_id,
            project_generation: request.project_generation,
            transition_epoch: response_epoch,
            resource,
            parent_listing,
        })
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[cfg(test)]
mod tests;
