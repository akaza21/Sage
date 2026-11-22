//! Settings › Harnesses › "Check npm for new harness versions".
//! Main webview only: the guard in `ipc_commands.rs` rejects browser WebViews.

use tauri::{Emitter, State};

use crate::state::AppState;

#[tauri::command]
pub(crate) fn harness_update_check(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .settings
        .harness_update_check()
        .map_err(|error| error.to_string())
}

/// Turning it on checks now; turning it off drops what earlier checks found,
/// so the harness rows read "not checked" and no update notice stays up.
#[tauri::command]
pub(crate) fn harness_set_update_check(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    let enabled = state
        .settings
        .set_harness_update_check(enabled)
        .map_err(|error| error.to_string())?;
    if enabled {
        AppState::refresh_harnesses_after_window_shown(app);
    } else {
        state.harnesses.forget_published_releases();
        let _ = app.emit(
            "harness-discovery-changed-v1",
            state.harnesses.definitions(),
        );
    }
    Ok(enabled)
}
