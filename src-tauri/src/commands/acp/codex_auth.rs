use tauri::State;

use crate::{
    acp::{
        bindings::codex::{CodexAuthDeadline, CodexDiscoveryRunError},
        intent::{AcpIpcError, AcpUnavailableReason, CodexAuthResponse, CodexAuthScope},
        service::CodexAuthAction,
    },
    state::AppState,
};

use super::{codex_discovery_error, internal_error, lock_acp_context, safe_message};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexAuthCommandRequest {
    #[serde(flatten)]
    scope: CodexAuthScope,
    consent: bool,
}

#[tauri::command]
pub async fn get_codex_auth(
    state: State<'_, AppState>,
    request: CodexAuthScope,
) -> Result<CodexAuthResponse, AcpIpcError> {
    run_codex_auth(&state, request, CodexAuthAction::Observe, false).await
}

#[tauri::command]
pub async fn start_codex_auth(
    state: State<'_, AppState>,
    request: CodexAuthCommandRequest,
) -> Result<CodexAuthResponse, AcpIpcError> {
    run_codex_auth(
        &state,
        request.scope,
        CodexAuthAction::Login,
        request.consent,
    )
    .await
}

#[tauri::command]
pub async fn cancel_codex_auth(
    state: State<'_, AppState>,
    request: CodexAuthScope,
) -> Result<CodexAuthResponse, AcpIpcError> {
    validate_auth_scope(&state, &request).await?;
    state
        .acp
        .codex_auth_manager()
        .cancel(request)
        .map_err(codex_discovery_error)
}

#[tauri::command]
pub async fn logout_codex_auth(
    state: State<'_, AppState>,
    request: CodexAuthScope,
) -> Result<CodexAuthResponse, AcpIpcError> {
    run_codex_auth(&state, request, CodexAuthAction::Logout, false).await
}

async fn run_codex_auth(
    state: &AppState,
    scope: CodexAuthScope,
    action: CodexAuthAction,
    consent: bool,
) -> Result<CodexAuthResponse, AcpIpcError> {
    let deadline = CodexAuthDeadline::new().map_err(codex_discovery_error)?;
    let transition = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline.operation_end()),
        lock_acp_context(state, &scope.project_id, scope.project_generation),
    )
    .await
    .map_err(|_| codex_discovery_error(CodexDiscoveryRunError::Timeout))??;
    let root = state
        .database
        .codex_project_root_authority(&transition)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex beta requires one unchanged Project root."),
        })?;
    let manager = state.acp.codex_auth_manager();
    let prepared = manager
        .prepare(scope, root, action, consent, deadline)
        .map_err(codex_discovery_error)?;
    if requires_pre_worker_invalidation(action) {
        state.acp.codex_discovery_manager().forget_snapshot();
    }
    drop(transition);
    let result = tauri::async_runtime::spawn_blocking(move || manager.run(prepared))
        .await
        .map_err(|_| internal_error())?
        .map_err(codex_discovery_error)?;
    state.acp.codex_discovery_manager().invalidate();
    Ok(result)
}

fn requires_pre_worker_invalidation(action: CodexAuthAction) -> bool {
    matches!(action, CodexAuthAction::Login | CodexAuthAction::Logout)
}

async fn validate_auth_scope(state: &AppState, scope: &CodexAuthScope) -> Result<(), AcpIpcError> {
    let _transition = lock_acp_context(state, &scope.project_id, scope.project_generation).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effectful_auth_actions_require_pre_worker_cache_invalidation() {
        assert!(!requires_pre_worker_invalidation(CodexAuthAction::Observe));
        assert!(requires_pre_worker_invalidation(CodexAuthAction::Login));
        assert!(requires_pre_worker_invalidation(CodexAuthAction::Logout));
    }
}
