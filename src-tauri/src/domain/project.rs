use serde::{Deserialize, Serialize};

use super::{
    ArchivedChatSummary, ChatMessage, FileChange, HarnessDefinition, ProjectLayoutRequest,
    ProjectRepositoryStatus, RecoveredEditorBuffer, ResourceRef, WorkbenchTab,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRailChat {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    pub position: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListItem {
    pub id: String,
    pub name: String,
    pub root_count: u32,
    pub last_opened_at: i64,
    #[serde(default)]
    pub rail_chats: Vec<AgentRailChat>,
    #[serde(default)]
    pub rail_chats_truncated: bool,
    #[serde(default)]
    pub rail_archived: Vec<ArchivedChatSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRootSummary {
    pub id: String,
    pub name: String,
    pub ordinal: u32,
    pub availability: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub roots: Vec<ProjectRootSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub name: String,
    pub resource: ResourceRef,
    pub kind: String,
    pub byte_len: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub resource: ResourceRef,
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSwitchRequest {
    pub target_project_id: String,
    pub current_layout: ProjectLayoutRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTransitionResult {
    pub committed: bool,
    pub transition_epoch: u64,
    pub snapshot: Option<BootstrapSnapshot>,
    pub recovery_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSnapshot {
    pub transition_epoch: u64,
    pub projects: Vec<ProjectListItem>,
    pub project: Option<ProjectSummary>,
    pub tabs: Vec<WorkbenchTab>,
    pub archived_chats: Vec<ArchivedChatSummary>,
    pub active_tab_id: Option<String>,
    pub selected_root_id: Option<String>,
    pub selected_resource: Option<ResourceRef>,
    pub harnesses: Vec<HarnessDefinition>,
    pub session_id: Option<String>,
    pub selected_harness_id: Option<String>,
    pub selected_model_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub repositories: ProjectRepositoryStatus,
    pub turn_changes: Vec<FileChange>,
    pub recovered_buffers: Vec<RecoveredEditorBuffer>,
    pub recovery_notice: Option<String>,
}
