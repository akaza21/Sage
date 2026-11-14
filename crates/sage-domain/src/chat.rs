use serde::{Deserialize, Serialize};

use crate::{repository::RepositoryRef, resource::ResourceRef};
use sage_acp_protocol::file_diff::DiffRef;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    /// When the chat's latest Turn finished, in Unix milliseconds. Read marks
    /// compare against it: parking a session or other runtime bookkeeping
    /// moves `updated_at` but not this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    pub resource: Option<ResourceRef>,
    #[serde(default)]
    pub repository: Option<RepositoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_ref: Option<DiffRef>,
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

/// Permanently deletes an archived Chat; no attached layout changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeleteRequest {
    pub project_id: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinChatRequest {
    pub project_id: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveChatRequest {
    pub project_id: String,
    pub chat_id: String,
    pub target_project_id: String,
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
