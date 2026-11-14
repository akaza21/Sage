use tauri::State;

use crate::{acp::service::runtime_identity::CodexRuntimeIdentityDiagnostics, state::AppState};

/// Not registered in release builds unless the `diagnostics` feature is enabled.
#[tauri::command]
pub fn get_codex_runtime_identity(
    state: State<'_, AppState>,
) -> Option<CodexRuntimeIdentityDiagnostics> {
    state.acp.codex_runtime_identity()
}
