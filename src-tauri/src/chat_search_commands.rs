use sage_core::error::{AppError, AppResult};
use tauri::State;

use crate::{
    chat_search::{
        ProjectChatSearchRequest, ProjectChatSearchResponse,
        search_project_chats as execute_project_chat_search, validate_chat_search_request,
    },
    state::AppState,
};

#[tauri::command]
pub async fn search_project_chats(
    state: State<'_, AppState>,
    request: ProjectChatSearchRequest,
) -> Result<ProjectChatSearchResponse, String> {
    search_project_chats_inner(&state, request)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn search_project_chats_inner(
    state: &AppState,
    request: ProjectChatSearchRequest,
) -> AppResult<ProjectChatSearchResponse> {
    validate_chat_search_request(&request)?;
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != request.expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "project changed before Chat search could run".into(),
        ));
    }
    require_existing_project(&state.database, &request.project_id)?;
    drop(transition_epoch);
    let database = state.database.clone();
    let response = tauri::async_runtime::spawn_blocking(move || {
        execute_project_chat_search(&database, &request)
    })
    .await
    .map_err(|_| AppError::InvalidRequest("Chat search worker stopped unexpectedly".into()))??;
    if response.transition_epoch != *state.project_transitions.lock().await {
        return Err(AppError::InvalidRequest(
            "project changed before Chat search completed".into(),
        ));
    }
    Ok(response)
}

fn require_existing_project(
    database: &crate::persistence::Database,
    project_id: &str,
) -> AppResult<()> {
    let exists: bool = database.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
        [project_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(AppError::InvalidRequest(
            "Chat search Project does not belong to this workspace".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
