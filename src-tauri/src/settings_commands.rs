use tauri::State;

use crate::{
    settings_store::{EditorAppearance, SessionAccessMode, UiAppearance},
    state::AppState,
};

#[tauri::command]
pub fn get_editor_appearance(state: State<'_, AppState>) -> Result<EditorAppearance, String> {
    state
        .settings
        .editor_appearance()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_editor_appearance(
    state: State<'_, AppState>,
    appearance: EditorAppearance,
) -> Result<EditorAppearance, String> {
    state
        .settings
        .set_editor_appearance(appearance)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_session_access_mode(state: State<'_, AppState>) -> Result<SessionAccessMode, String> {
    state
        .settings
        .session_access_mode()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_session_access_mode(
    state: State<'_, AppState>,
    mode: SessionAccessMode,
) -> Result<SessionAccessMode, String> {
    state
        .settings
        .set_session_access_mode(mode)
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
