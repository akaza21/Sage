//! Debug-only native identity observation command.

use tauri::State;

use crate::{acp::service::runtime_identity::CodexRuntimeIdentityDiagnostics, state::AppState};

/// `get_codex_runtime_identity({}) -> identity | null`
///
/// This is intentionally read-only and returns `null` before an exact live
/// Codex runtime has passed publication. It is not registered in normal
/// release builds unless the explicit `diagnostics` feature is enabled.
#[tauri::command]
pub fn get_codex_runtime_identity(
    state: State<'_, AppState>,
) -> Option<CodexRuntimeIdentityDiagnostics> {
    state.acp.codex_runtime_identity()
}
