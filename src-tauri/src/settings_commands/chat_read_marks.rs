//! The Agent rail's read marks. Like the other settings commands they run
//! only for the main webview; the store validates and bounds every mark.

use tauri::State;

use crate::{settings_store::ChatReadMark, state::AppState};

#[tauri::command]
pub(crate) fn chat_read_marks(state: State<'_, AppState>) -> Result<Vec<ChatReadMark>, String> {
    state
        .settings
        .chat_read_marks()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn chat_mark_read(
    state: State<'_, AppState>,
    mark: ChatReadMark,
) -> Result<ChatReadMark, String> {
    state
        .settings
        .mark_chat_read(&mark)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn chat_mark_unread(state: State<'_, AppState>, chat_id: String) -> Result<(), String> {
    state
        .settings
        .mark_chat_unread(&chat_id)
        .map_err(|error| error.to_string())
}
