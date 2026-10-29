//! Browser settings commands (BF-7d). They run only for the main webview: the
//! invoke guard in `ipc_commands.rs` rejects every browser tab before any
//! command runs. The record is closed (`deny_unknown_fields`), so a renderer
//! can store nothing but the listed choices, and no reply carries a URL,
//! cookie or password.

use tauri::State;

use crate::{settings_store::BrowserPreferences, state::AppState};

#[tauri::command]
pub(crate) fn browser_preferences(
    state: State<'_, AppState>,
) -> Result<BrowserPreferences, String> {
    state
        .settings
        .browser_preferences()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn browser_set_preferences(
    state: State<'_, AppState>,
    preferences: BrowserPreferences,
) -> Result<BrowserPreferences, String> {
    state
        .settings
        .set_browser_preferences(preferences)
        .map_err(|error| error.to_string())
}
