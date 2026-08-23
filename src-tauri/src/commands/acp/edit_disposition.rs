//! Owner-landed fail-closed disposition command. AGENT-1 implements the store write.

use crate::acp::{
    contract::{EntityRevision, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    intent::{AcpIpcError, AcpUnavailableReason},
    store::edit_disposition::{
        AgentEditDisposition, AgentEditDispositionState,
        record_agent_edit_disposition as persist_agent_edit_disposition,
    },
};

use super::errors::safe_message;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordAgentEditDispositionRequest {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub expected_revision: EntityRevision,
    pub state: AgentEditDispositionState,
}

#[tauri::command]
pub async fn record_agent_edit_disposition(
    request: RecordAgentEditDispositionRequest,
) -> Result<serde_json::Value, AcpIpcError> {
    persist_agent_edit_disposition(&AgentEditDisposition {
        project_id: request.project_id,
        chat_id: request.chat_id,
        session_id: request.session_id,
        turn_id: request.turn_id,
        expected_revision: request.expected_revision,
        state: request.state,
    })
    .map_err(|_| AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::CapabilityUnavailable,
        message: safe_message("Agent edit disposition is not implemented."),
    })?;
    Ok(serde_json::json!({ "ok": true }))
}
