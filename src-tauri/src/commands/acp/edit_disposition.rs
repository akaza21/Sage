//! Owner-landed fail-closed disposition command. AGENT-1 implements the store write.

use tauri::State;

use crate::{
    acp::{
        contract::{EntityRevision, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
        intent::AcpIpcError,
        store::edit_disposition::{
            AgentEditDisposition, AgentEditDispositionState, record_agent_edit_disposition_in,
        },
    },
    state::AppState,
};

use super::errors::store_error;

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
    state: State<'_, AppState>,
    request: RecordAgentEditDispositionRequest,
) -> Result<serde_json::Value, AcpIpcError> {
    let mut connection = state.database.connection();
    record_agent_edit_disposition_in(
        &mut connection,
        &AgentEditDisposition {
            project_id: request.project_id,
            chat_id: request.chat_id,
            session_id: request.session_id,
            turn_id: request.turn_id,
            expected_revision: request.expected_revision,
            state: request.state,
        },
        crate::persistence::now_millis(),
    )
    .map_err(store_error)?;
    Ok(serde_json::json!({ "ok": true }))
}
