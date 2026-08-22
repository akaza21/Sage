use serde::{Deserialize, Serialize};

use super::{RepositoryRef, ResourceRef};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchTab {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub position: i64,
    pub pinned: bool,
    pub dirty: bool,
    pub status: Option<String>,
    pub resource: Option<ResourceRef>,
    #[serde(default)]
    pub repository: Option<RepositoryRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLayoutRequest {
    pub project_id: String,
    pub tabs: Vec<WorkbenchTab>,
    pub active_tab_id: Option<String>,
    pub selected_root_id: Option<String>,
    pub selected_resource: Option<ResourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameChatRequest {
    pub project_id: String,
    pub chat_id: String,
    pub title: String,
    pub current_layout: ProjectLayoutRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatLifecycleRequest {
    pub project_id: String,
    pub chat_id: String,
    pub current_layout: ProjectLayoutRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedChatSummary {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub role: String,
    pub text: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatorTurnResult {
    pub session_id: String,
    pub messages: Vec<ChatMessage>,
}
