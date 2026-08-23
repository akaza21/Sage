//! Owner-landed permission-decision command stub. AGENT-1 replaces the body.

use tauri::State;

use crate::{
    acp::{
        contract::{SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId},
        intent::{AcpIpcError, AcpUnavailableReason},
    },
    state::AppState,
};

use super::errors::safe_message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDecision {
    Ask,
    AllowOnce,
    Deny,
}

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
    _state: State<'_, AppState>,
    _request: ResolvePermissionDecisionRequest,
) -> Result<serde_json::Value, AcpIpcError> {
    Err(AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::CapabilityUnavailable,
        message: safe_message("Durable permission decisions are not implemented."),
    })
}
