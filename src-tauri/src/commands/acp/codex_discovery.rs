use super::*;
use crate::acp::bindings::codex::runtime::authority::CodexRootAuthorityStore;
use crate::acp::{
    bindings::codex::{CodexDiscoveryDeadline, CodexDiscoveryRunError},
    ipc::{AcpUnavailableReason, CodexDiscoveryResponse, GetCodexDiscoveryRequest},
};

#[tauri::command]
pub async fn get_codex_discovery(
    state: State<'_, AppState>,
    request: GetCodexDiscoveryRequest,
) -> Result<CodexDiscoveryResponse, crate::acp::ipc::AccountReadFailure> {
    let account_read_started_at = sage_core::time::now_millis();
    get_codex_discovery_inner(&state, request)
        .await
        .map_err(|failure| crate::acp::ipc::AccountReadFailure {
            failure,
            account_read_started_at,
        })
}

pub(crate) async fn get_codex_discovery_inner(
    state: &AppState,
    request: GetCodexDiscoveryRequest,
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    let deadline = CodexDiscoveryDeadline::new().map_err(codex_discovery_error)?;
    get_codex_discovery_with_deadline(state, request, deadline).await
}

pub(crate) async fn get_codex_discovery_with_deadline(
    state: &AppState,
    request: GetCodexDiscoveryRequest,
    deadline: CodexDiscoveryDeadline,
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    let transition = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline.operation_end()),
        lock_acp_context(state, &request.project_id, request.project_generation),
    )
    .await
    .map_err(|_| codex_discovery_error(CodexDiscoveryRunError::Timeout))??;
    let project_root = state
        .database
        .codex_project_root_authority(&transition)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex beta requires one unchanged Project root."),
        })?;
    let manager = state.acp.codex_discovery_manager();
    let prepared = manager
        .prepare(request, project_root, deadline)
        .map_err(codex_discovery_error)?;
    drop(transition);
    let on_timeout = manager.cancellation(&prepared);
    let task = tauri::async_runtime::spawn_blocking(move || manager.discover(prepared));
    await_codex_discovery(task, deadline, on_timeout).await
}

pub(crate) async fn await_codex_discovery(
    task: tauri::async_runtime::JoinHandle<
        Result<CodexDiscoveryResponse, crate::acp::bindings::codex::CodexDiscoveryRunError>,
    >,
    deadline: CodexDiscoveryDeadline,
    on_timeout: impl FnOnce(),
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline.cleanup_end()), task)
        .await
    {
        Ok(result) => result
            .map_err(|_| internal_error())?
            .map_err(codex_discovery_error),
        Err(_) => {
            on_timeout();
            Err(codex_discovery_error(CodexDiscoveryRunError::Timeout))
        }
    }
}
