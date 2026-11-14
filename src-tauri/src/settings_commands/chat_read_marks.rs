use tauri::State;

use crate::{
    chat_read_state::ChatAttentionList,
    settings_store::{ChatReadMark, ChatReadMarkWrite, ChatReadMarks},
    state::AppState,
};

#[tauri::command]
pub(crate) fn chat_read_marks(state: State<'_, AppState>) -> Result<ChatReadMarks, String> {
    state
        .settings
        .chat_read_marks()
        .map_err(|error| error.to_string())
}

/// Finding the rail's chats reads the project database, so it runs off the
/// main thread.
#[tauri::command]
pub(crate) async fn chat_mark_read(
    state: State<'_, AppState>,
    mark: ChatReadMark,
) -> Result<ChatReadMarkWrite, String> {
    let database = state.database.clone();
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::chat_read_state::mark_read(&database, &settings, &mark)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn chat_mark_unread(state: State<'_, AppState>, chat_id: String) -> Result<(), String> {
    state
        .settings
        .mark_chat_unread(&chat_id)
        .map_err(|error| error.to_string())
}

/// Reads every project's chats, so it runs off the main thread.
#[tauri::command]
pub(crate) async fn chat_attention(
    state: State<'_, AppState>,
) -> Result<ChatAttentionList, String> {
    let database = state.database.clone();
    let settings = state.settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::chat_read_state::chat_attention(&database, &settings)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}
