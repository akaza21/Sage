use tauri::State;

pub(crate) mod browser_preferences;

use crate::{
    settings_store::{EditorSettings, SessionAccessModes, UiAppearance},
    state::AppState,
};

pub(crate) fn access_preferences(state: &AppState) -> crate::error::AppResult<SessionAccessModes> {
    if !state.settings.pending_access_migration()? {
        return state.settings.session_access_modes();
    }
    let snapshot = state
        .database
        .bootstrap_snapshot_with_catalog(&state.harnesses)?;
    let binding = snapshot
        .session_id
        .and(snapshot.selected_harness_id)
        .and_then(|id| {
            state
                .acp
                .binding_registry()
                .for_harness(&id)
                .map(|record| record.binding_id())
        });
    state.settings.migrate_session_access_modes(binding)
}

#[tauri::command]
pub fn get_editor_settings(state: State<'_, AppState>) -> Result<EditorSettings, String> {
    state
        .settings
        .editor_settings()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_editor_settings(
    state: State<'_, AppState>,
    changes: std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<EditorSettings, String> {
    state
        .settings
        .set_editor_settings(&changes)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_session_access_modes(state: State<'_, AppState>) -> Result<SessionAccessModes, String> {
    access_preferences(&state).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_session_access_modes(
    state: State<'_, AppState>,
    modes: SessionAccessModes,
) -> Result<SessionAccessModes, String> {
    state
        .settings
        .set_session_access_modes(&modes)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_ui_appearance(state: State<'_, AppState>) -> Result<UiAppearance, String> {
    state
        .settings
        .ui_appearance()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_ui_appearance(
    state: State<'_, AppState>,
    appearance: UiAppearance,
) -> Result<UiAppearance, String> {
    state
        .settings
        .set_ui_appearance(appearance)
        .map_err(|error| error.to_string())
}
