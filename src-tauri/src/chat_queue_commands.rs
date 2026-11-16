use std::collections::HashMap;

use tauri::State;

use crate::chat_queue::ChatQueueStore;
use crate::{chat_queue::DurableQueuedPrompt, state::AppState};

#[tauri::command]
pub async fn get_project_chat_queued_prompts(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<HashMap<String, Vec<DurableQueuedPrompt>>, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.project_chat_queued_prompts(&project_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn replace_chat_queued_prompts(
    state: State<'_, AppState>,
    project_id: String,
    chat_id: String,
    prompts: Vec<DurableQueuedPrompt>,
) -> Result<(), String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.replace_chat_queued_prompts(&project_id, &chat_id, &prompts)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}
