use serde::Serialize;

mod browser_operation;
mod session_activity;
pub use browser_operation::BrowserOperationProjection;
pub use session_activity::TurnSessionActivityProjection;

use super::{
    contract::*,
    declared_test::CommandActivity,
    intent::{
        BindingId, COMMAND_RESOURCE_LIMIT, CapabilitySnapshotV1, ImageSource, ModelId, PageCursor,
        PermissionOptionKind, ToolCategory,
    },
};

pub use super::intent::{SafeResourceLocation, UnavailableResourceReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemOrigin {
    Primary,
    HarnessActivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionRequestSource {
    Harness,
    Browser,
    /// A developer-mode card (ADR-0009b).
    BrowserDeveloper,
}

impl PermissionRequestSource {
    /// The card's heading in the transcript.
    pub fn label(self) -> &'static str {
        match self {
            Self::Browser => "Browser permission",
            Self::BrowserDeveloper => "Browser developer mode",
            Self::Harness => "Permission request",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanEntryState {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub label: SafeText<512>,
    pub state: PlanEntryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ToolPresentation {
    Read {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    },
    Edit {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    },
    Delete {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    },
    Move {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        original_resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    },
    Search {
        label: SafeText<512>,
        query_summary: SafeText<16384>,
        resources: BoundedVec<SafeResourceLocation, 256>,
        match_count: Option<u32>,
        truncated: bool,
    },
    Execute {
        label: SafeText<512>,
        summary: Option<SafeText<16384>>,
        cwd: Option<SafeResourceLocation>,
        exit_code: Option<i32>,
        output_tail: Option<SafeText<16384>>,
        output_truncated: bool,
        resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        #[serde(default)]
        activity: CommandActivity,
    },
    Fetch {
        label: SafeText<512>,
        url: Option<SafeText<4096>>,
        summary: Option<SafeText<16384>>,
    },
    WebSearch {
        label: SafeText<512>,
    },
    ImageView {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    },
    Think {
        label: SafeText<512>,
        summary: Option<SafeText<16384>>,
    },
    SwitchMode {
        label: SafeText<512>,
        mode_id: Option<SafeText<512>>,
        mode_label: Option<SafeText<512>>,
    },
    Mcp {
        label: SafeText<512>,
        server_label: SafeText<512>,
        tool_label: SafeText<512>,
        arguments_summary: Option<SafeText<16384>>,
        result_summary: Option<SafeText<16384>>,
        /// Latest status line of a running call; absent once it is terminal.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress: Option<SafeText<{ super::intent::TOOL_PROGRESS_BYTES }>>,
    },
    Subagent {
        label: SafeText<512>,
        activity_id: Option<SageActivityId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_lifecycle: Option<bool>,
    },
    Other {
        label: SafeText<512>,
        raw_kind: Option<SafeText<128>>,
        summary: Option<SafeText<16384>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOperation {
    Created,
    Updated,
    Deleted,
    Renamed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FilePairState {
    /// An immutable snapshot revision is retained for the narrow read.
    Available,
    /// No pair was admitted (legacy summary, harness without pair evidence,
    /// or unavailable resource). Never fabricated text.
    Unavailable,
    /// Over-cap admission: summary only with truthful truncation.
    SummaryTruncated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeProjection {
    pub resource: SafeResourceLocation,
    pub operation: FileOperation,
    pub original_resource: Option<SafeResourceLocation>,
    pub summary: SafeText<65536>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub pair_state: FilePairState,
    /// Immutable snapshot revision; zero unless `pair_state == Available`.
    #[serde(with = "crate::acp::file_diff::wire_counter")]
    pub snapshot_revision: u64,
    /// Raw UTF-8 byte lengths of the retained pair, when available.
    pub old_bytes: Option<u64>,
    pub new_bytes: Option<u64>,
    /// 64-char pair digest binding the immutable bytes, when available.
    pub content_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ItemContent {
    Text {
        role: TextRole,
        text: SafeText<262144>,
    },
    ThoughtSummary {
        summary: SafeText<262144>,
    },
    Plan {
        entries: BoundedVec<PlanEntry, 128>,
    },
    CompactionNotice {},
    ToolCall {
        presentation: ToolPresentation,
        input_summary: Option<SafeText<65536>>,
        output_summary: Option<SafeText<65536>>,
    },
    FileChangeDiff {
        changes: BoundedVec<FileChangeProjection, 32>,
        truncated: bool,
    },
    Image {
        media_type: crate::acp::intent::ImageMediaType,
        data_url: SafeText<262144>,
        source: ImageSource,
    },
    PermissionRequest {
        request_id: SageServerRequestId,
        state: ServerRequestState,
        label: SafeText<512>,
        summary: SafeText<65536>,
        source: PermissionRequestSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        offered_option_kinds: Option<BoundedVec<PermissionOptionKind, 8>>,
        category: ToolCategory,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<SafeText<2048>>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<super::intent::ErrorKind>,
        code: SafeText<128>,
        message: SafeText<8192>,
        recoverable: bool,
    },
    Unsupported {
        category: SafeText<128>,
        summary: SafeText<16384>,
        truncated: bool,
    },
    Collaboration {
        activity_id: Option<SageActivityId>,
        label: SafeText<512>,
        summary: SafeText<16384>,
        receiver_labels: BoundedVec<SafeText<512>, 32>,
        operation: Option<SafeText<32>>,
        agent_key: Option<SafeText<512>>,
        agent_keys: BoundedVec<SafeText<512>, 32>,
        agent_states: BoundedVec<(SafeText<512>, SafeText<32>), 32>,
    },
}

impl ItemContent {
    pub fn requires_tool_state(&self) -> bool {
        matches!(self, Self::ToolCall { .. } | Self::PermissionRequest { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemProjection {
    pub id: SageItemId,
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub ordinal: u64,
    pub origin: ItemOrigin,
    pub activity_id: Option<SageActivityId>,
    pub lifecycle: ItemLifecycle,
    pub revision: EntityRevision,
    pub created_at: i64,
    pub updated_at: i64,
    pub terminal_at: Option<i64>,
    pub tool_state: Option<ToolState>,
    #[serde(flatten)]
    pub content: ItemContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityRole {
    Root,
    Fork,
    Child,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityProjection {
    pub id: SageActivityId,
    pub turn_id: SageTurnId,
    pub parent_activity_id: Option<SageActivityId>,
    pub parent_item_id: SageItemId,
    pub role: ActivityRole,
    pub lifecycle: ActivityLifecycle,
    pub depth: u8,
    pub provenance: super::intent::ActivityProvenanceV1,
    pub revision: EntityRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnProjection {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attached_context: Vec<super::attached_context::AttachedContext>,
    pub id: SageTurnId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub ordinal: u64,
    pub state: TurnState,
    pub revision: EntityRevision,
    pub created_at: i64,
    pub updated_at: i64,
    pub terminal_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TurnUsageProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<TurnTerminalReasonProjection>,
    pub items: BoundedVec<ItemProjection, 256>,
    pub activities: BoundedVec<ActivityProjection, 32>,
    pub browser_operations: BoundedVec<BrowserOperationProjection, 32>,
    pub browser_operations_collapsed: u64,
    /// At most the session surface's activity cap.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub session_activities: Vec<TurnSessionActivityProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsageProjection {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTerminalReasonProjection {
    pub kind: TurnTerminalReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProjection {
    pub id: SageAgentSessionId,
    pub ordinal: u64,
    pub binding_id: BindingId,
    pub model_id: Option<ModelId>,
    pub state: SessionState,
    pub residency: Residency,
    pub runtime_epoch: Option<RuntimeEpoch>,
    /// Exact epoch a cold created/ready session must present for activation.
    /// Null while an epoch is open or the session cannot be activated.
    pub next_runtime_epoch: Option<RuntimeEpoch>,
    pub capabilities: CapabilitySnapshotV1,
    pub permission_requests: bool,
    pub revision: EntityRevision,
    pub projection_revision: ProjectionRevision,
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpChatPage {
    pub(crate) session_config: Option<super::session_config::SessionConfigOverlay>,
    pub(crate) goal: super::goal::GoalState,
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub title: SafeText<8192>,
    pub projection_revision: ProjectionRevision,
    pub current_session_id: Option<SageAgentSessionId>,
    pub sessions: BoundedVec<SessionProjection, 32>,
    pub turns: BoundedVec<TurnProjection, 20>,
    pub next_cursor: Option<PageCursor>,
    /// The Chat this one was natively forked from (durable `chat_lineage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) forked_from: Option<SageChatId>,
    /// The parent Turn the fork carries history through, inclusive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) forked_from_turn_id: Option<SageTurnId>,
    /// Child-transcript invalidation epoch: advances on every live child
    /// change and never moves `projection_revision` (see
    /// `store::child_transcript::child_projection_epoch`).
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) child_projection_epoch: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

mod validation;
