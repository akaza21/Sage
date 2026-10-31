use crate::{
    chat_attention::{
        contract::{AckAttention, ChatAttentionV1, GetAttention, MarkUnread},
        read_state, store,
    },
    state::AppState,
};
use tauri::State;

#[tauri::command]
pub(crate) fn get_chat_attention(
    state: State<'_, AppState>,
    request: GetAttention,
) -> Result<Vec<ChatAttentionV1>, String> {
    let chats = request
        .chat_ids
        .as_slice()
        .iter()
        .map(|chat| chat.as_str().to_owned())
        .collect::<Vec<_>>();
    store::get(
        &state.database.connection(),
        request.project_id.as_str(),
        &chats,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn ack_chat_attention(
    state: State<'_, AppState>,
    request: AckAttention,
) -> Result<ChatAttentionV1, String> {
    let mut connection = state
        .database
        .write_connection()
        .map_err(|error| error.to_string())?;
    read_state::ack(
        &mut connection,
        request.project_id.as_str(),
        request.chat_id.as_str(),
        request.observed_position,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn mark_chat_unread(
    state: State<'_, AppState>,
    request: MarkUnread,
) -> Result<ChatAttentionV1, String> {
    let mut connection = state
        .database
        .write_connection()
        .map_err(|error| error.to_string())?;
    read_state::mark_unread(
        &mut connection,
        request.project_id.as_str(),
        request.chat_id.as_str(),
        request.expected_revision,
    )
    .map_err(|error| error.to_string())
}
