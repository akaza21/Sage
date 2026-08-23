//! Owner-landed fail-closed stub. AGENT-1 replaces the body.

use crate::acp::contract::{
    EntityRevision, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentEditDisposition {
    pub(crate) project_id: SageProjectId,
    pub(crate) chat_id: SageChatId,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) turn_id: SageTurnId,
    pub(crate) expected_revision: EntityRevision,
    pub(crate) state: AgentEditDispositionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AgentEditDispositionState {
    Accepted,
    Rejected,
}

pub(crate) fn record_agent_edit_disposition(
    _disposition: &AgentEditDisposition,
) -> Result<(), String> {
    Err("agent edit disposition is not implemented".into())
}
