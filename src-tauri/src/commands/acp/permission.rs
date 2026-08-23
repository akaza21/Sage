//! Owner-landed permission-decision command stub. AGENT-1 replaces the body.

use tauri::State;

use crate::{
    acp::runtime::permissions::PermissionDecision,
    acp::{
        contract::{SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId},
        intent::{AcpIpcError, AcpUnavailableReason},
    },
    state::AppState,
};

use super::errors::safe_message;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvePermissionDecisionRequest {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub request_id: SageServerRequestId,
    pub decision: PermissionDecision,
}

#[tauri::command]
pub async fn resolve_permission_decision(
    state: State<'_, AppState>,
    request: ResolvePermissionDecisionRequest,
) -> Result<serde_json::Value, AcpIpcError> {
    let response = state
        .acp
        .resolve_permission_decision(
            request.project_id,
            request.chat_id,
            request.session_id,
            request.request_id,
            request.decision,
        )
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("The permission runtime is unavailable."),
        })?;
    serde_json::to_value(response).map_err(|_| AcpIpcError::Internal {
        message: safe_message("The permission response could not be encoded."),
    })
}
