//! The Agent rail's read marks, and the cross-project attention they feed.
//! Like the other settings commands they run only for the main webview; the
//! store validates and bounds every mark.

use tauri::State;

use crate::{chat_read_state::ChatAttentionList, settings_store::ChatReadMark, state::AppState};

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

/// Chats in any project that wait on an approval or finished unseen. It
/// reads every project's chats, so it runs off the main thread.
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
