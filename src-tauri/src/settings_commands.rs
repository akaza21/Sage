use tauri::State;

use crate::{settings_store::EditorAppearance, state::AppState};

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
