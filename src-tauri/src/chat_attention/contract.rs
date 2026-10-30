use serde::{Deserialize, Serialize};

use crate::acp::contract::{
    SageAgentSessionId, SageChatId, SageProjectId, SessionState, TurnState,
};

pub(crate) use crate::acp::contract::AttentionPosition as Position;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatAttentionV1 {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: Option<SageAgentSessionId>,
    pub session_state: Option<SessionState>,
    pub turn_state: Option<TurnState>,
    pub running: bool,
    pub pending_approvals: u32,
    pub pending_requests: u32,
    pub activity_position: Position,
    pub read_position: Option<Position>,
    pub terminal_position: Option<Position>,
    pub terminal_state: Option<TurnState>,
    pub forced_unread_position: Option<Position>,
    pub revision: Position,
    pub lineage: Option<AttentionLineage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttentionLineage {
    pub parent_project_id: SageProjectId,
    pub parent_chat_id: SageChatId,
    pub relation: AttentionRelation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AttentionRelation {
    NativeFork,
    SideChat,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GetAttention {
    pub project_id: SageProjectId,
    pub chat_ids: crate::acp::contract::BoundedVec<SageChatId, 64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AckAttention {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub observed_position: Position,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MarkUnread {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub expected_revision: Position,
}
