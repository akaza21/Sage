use serde::{Deserialize, Serialize};

use sage_acp_protocol::contract::{SageAgentSessionId, SageChatId, SageProjectId, SageTurnId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOperationProjection {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub tab_id: String,
    pub operation_id: String,
    pub operation: String,
    pub phase: String,
    pub origin: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
