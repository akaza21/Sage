use serde::Serialize;

use super::{
    contract::*,
    intent::{BindingId, CapabilitySnapshotV1, ModelId, PageCursor, SafeResourceRef},
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
    Generic {
        label: SafeText<512>,
        summary: Option<SafeText<16384>>,
    },
    File {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, 256>,
    },
    Diff {
        label: SafeText<512>,
        resources: BoundedVec<SafeResourceLocation, 256>,
    },
    Command {
        label: SafeText<512>,
        summary: Option<SafeText<16384>>,
    },
    Search {
        label: SafeText<512>,
        query_summary: SafeText<16384>,
        resources: BoundedVec<SafeResourceRef, 256>,
    },
    Web {
        label: SafeText<512>,
        summary: Option<SafeText<16384>>,
    },
    Mcp {
        label: SafeText<512>,
        server_label: SafeText<512>,
        tool_label: SafeText<512>,
    },
    Subagent {
        label: SafeText<512>,
        activity_id: Option<SageActivityId>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeProjection {
    pub resource: SafeResourceLocation,
    pub operation: FileOperation,
    pub original_resource: Option<SafeResourceLocation>,
    pub summary: SafeText<65536>,
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
    ToolCall {
        presentation: ToolPresentation,
        input_summary: Option<SafeText<65536>>,
    },
    ToolUpdateResult {
        call_item_id: Option<SageItemId>,
        presentation: ToolPresentation,
        output_summary: Option<SafeText<65536>>,
    },
    FileChangeDiff {
        changes: BoundedVec<FileChangeProjection, 256>,
        truncated: bool,
    },
    Usage {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cached_input_tokens: Option<u64>,
        total_tokens: Option<u64>,
    },
    PermissionRequest {
        request_id: SageServerRequestId,
        state: ServerRequestState,
        label: SafeText<512>,
        summary: SafeText<65536>,
    },
    Error {
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
        activity_id: SageActivityId,
        label: SafeText<512>,
        summary: SafeText<16384>,
        receiver_labels: BoundedVec<SafeText<512>, 32>,
    },
}

impl ItemContent {
    pub fn requires_tool_state(&self) -> bool {
        matches!(
            self,
            Self::ToolCall { .. } | Self::ToolUpdateResult { .. } | Self::PermissionRequest { .. }
        )
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
    pub id: SageTurnId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub ordinal: u64,
    pub state: TurnState,
    pub revision: EntityRevision,
    pub created_at: i64,
    pub updated_at: i64,
    pub terminal_at: Option<i64>,
    pub items: BoundedVec<ItemProjection, 256>,
    pub activities: BoundedVec<ActivityProjection, 32>,
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
    pub revision: EntityRevision,
    pub projection_revision: ProjectionRevision,
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpChatPage {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub title: SafeText<8192>,
    pub projection_revision: ProjectionRevision,
    pub current_session_id: Option<SageAgentSessionId>,
    pub sessions: BoundedVec<SessionProjection, 32>,
    pub turns: BoundedVec<TurnProjection, 20>,
    pub next_cursor: Option<PageCursor>,
}

impl AcpChatPage {
    pub fn validate_page_bounds(&self) -> Result<(), ContractError> {
        let mut items = 0_usize;
        let mut activities = 0_usize;
        if self.current_session_id.as_ref().is_some_and(|current| {
            !self
                .sessions
                .as_slice()
                .iter()
                .any(|session| &session.id == current)
        }) {
            return Err(ContractError::ForbiddenContent {
                field: "currentSessionId",
            });
        }
        for turn in self.turns.as_slice() {
            if !self
                .sessions
                .as_slice()
                .iter()
                .any(|session| session.id == turn.session_id)
            {
                return Err(ContractError::ForbiddenContent {
                    field: "turn.sessionId",
                });
            }
            for item in turn.items.as_slice() {
                if item.project_id != self.project_id
                    || item.chat_id != self.chat_id
                    || item.session_id != turn.session_id
                    || item.turn_id != turn.id
                    || item.content.requires_tool_state() != item.tool_state.is_some()
                {
                    return Err(ContractError::ForbiddenContent {
                        field: "itemOwnership",
                    });
                }
            }
            for activity in turn.activities.as_slice() {
                if activity.turn_id != turn.id {
                    return Err(ContractError::ForbiddenContent {
                        field: "activityOwnership",
                    });
                }
            }
            items = items.checked_add(turn.items.as_slice().len()).ok_or(
                ContractError::TooManyEntries {
                    field: "historyPageItems",
                    actual: usize::MAX,
                    maximum: 256,
                },
            )?;
            activities = activities
                .checked_add(turn.activities.as_slice().len())
                .ok_or(ContractError::TooManyEntries {
                    field: "activityNodesPerPage",
                    actual: usize::MAX,
                    maximum: 128,
                })?;
        }
        if items > 256 {
            return Err(ContractError::TooManyEntries {
                field: "historyPageItems",
                actual: items,
                maximum: 256,
            });
        }
        if activities > 128 {
            return Err(ContractError::TooManyEntries {
                field: "activityNodesPerPage",
                actual: activities,
                maximum: 128,
            });
        }
        Ok(())
    }
}
