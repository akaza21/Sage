//! The record is closed, so the renderer can store nothing but the two listed
//! choices.
//! Main webview only: the guard in `ipc_commands.rs` rejects browser WebViews.

use tauri::State;

use crate::{settings_store::AgentRailView, state::AppState};

#[tauri::command]
pub(crate) fn agent_rail_view(state: State<'_, AppState>) -> Result<AgentRailView, String> {
    state
        .settings
        .agent_rail_view()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn agent_rail_set_view(
    state: State<'_, AppState>,
    view: AgentRailView,
) -> Result<AgentRailView, String> {
    state
        .settings
        .set_agent_rail_view(view)
        .map_err(|error| error.to_string())
}
