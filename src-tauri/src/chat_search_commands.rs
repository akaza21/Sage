use tauri::State;

use crate::{
    chat_search::{
        ProjectChatSearchRequest, ProjectChatSearchResponse,
        search_project_chats as execute_project_chat_search, validate_chat_search_request,
    },
    error::{AppError, AppResult},
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
    if state.database.active_project_id()?.as_deref() != Some(&request.project_id) {
        return Err(AppError::InvalidRequest(
            "Chat search Project is not active".into(),
        ));
    }
    let database = state.database.clone();
    let response = tauri::async_runtime::spawn_blocking(move || {
        execute_project_chat_search(&database, &request)
    })
    .await
    .map_err(|_| AppError::InvalidRequest("Chat search worker stopped unexpectedly".into()))??;
    if response.transition_epoch != *transition_epoch {
        return Err(AppError::InvalidRequest(
            "project changed before Chat search completed".into(),
        ));
    }
    Ok(response)
}

#[cfg(test)]
mod tests;
