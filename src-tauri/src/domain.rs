use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    pub root_id: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListItem {
    pub id: String,
    pub name: String,
    pub root_count: u32,
    pub last_opened_at: i64,
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
pub struct WorkbenchTab {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub position: i64,
    pub pinned: bool,
    pub dirty: bool,
    pub status: Option<String>,
    pub resource: Option<ResourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub resource: ResourceRef,
    pub original_resource: Option<ResourceRef>,
    pub index_status: String,
    pub worktree_status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootRepositoryStatus {
    pub root_id: String,
    pub state: String,
    pub branch: Option<String>,
    pub changes: Vec<FileChange>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    pub message: Option<String>,
}

impl RootRepositoryStatus {
    pub fn unavailable(root_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            root_id: root_id.into(),
            state: "unavailable".into(),
            branch: None,
            changes: Vec::new(),
            truncated: false,
            elapsed_ms: 0,
            message: Some(message.into()),
        }
    }

    pub fn not_repository(root_id: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            root_id: root_id.into(),
            state: "notRepository".into(),
            branch: None,
            changes: Vec::new(),
            truncated: false,
            elapsed_ms,
            message: None,
        }
    }

    pub fn repository_subdirectory(root_id: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            root_id: root_id.into(),
            state: "notRepository".into(),
            branch: None,
            changes: Vec::new(),
            truncated: false,
            elapsed_ms,
            message: Some(
                "This folder is inside a parent Git repository; select the repository top level for Source Control."
                    .into(),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRepositoryStatus {
    pub roots: Vec<RootRepositoryStatus>,
    pub truncated: bool,
    pub elapsed_ms: u64,
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
pub struct TextFile {
    pub resource: ResourceRef,
    pub content: String,
    pub byte_len: u64,
    pub line_ending: String,
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVersion {
    pub fingerprint: String,
    pub byte_len: u64,
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorBufferVersion {
    pub fingerprint: String,
    pub byte_len: u64,
    pub revision: u64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorConflict {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDocument {
    pub resource: ResourceRef,
    pub disk: DocumentVersion,
    pub base: DocumentVersion,
    pub buffer: EditorBufferVersion,
    pub line_ending: String,
    pub dirty: bool,
    pub conflict: Option<EditorConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSnapshotRequest {
    pub resource: ResourceRef,
    pub base: DocumentVersion,
    pub line_ending: String,
    pub content: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSnapshotAck {
    pub revision: u64,
    pub persisted_at: u64,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSaveResult {
    pub outcome: String,
    pub document: EditorDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredEditorBuffer {
    pub resource: ResourceRef,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub resource: ResourceRef,
    pub hunks: Vec<DiffHunk>,
    pub binary: bool,
    pub truncated: bool,
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
pub struct ProjectSwitchRequest {
    pub target_project_id: String,
    pub current_layout: ProjectLayoutRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTransitionResult {
    pub committed: bool,
    pub snapshot: Option<BootstrapSnapshot>,
    pub recovery_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub install_state: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
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
pub struct BootstrapSnapshot {
    pub projects: Vec<ProjectListItem>,
    pub project: Option<ProjectSummary>,
    pub tabs: Vec<WorkbenchTab>,
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
