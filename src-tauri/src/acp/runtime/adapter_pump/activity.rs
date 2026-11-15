use crate::acp::projection::ActivityRole;
use sage_acp_protocol::{
    contract::RequiredText,
    intent::{ActivityProvenanceV1, ExternalId, NormalizedPayload},
    lifecycle::{ActivityLifecycle, ItemLifecycle, ToolState},
};

use super::TurnScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurableActivityItem {
    pub(crate) scope: TurnScope,
    pub(crate) external_activity_id: ExternalId,
    pub(crate) external_item_id: ExternalId,
    pub(crate) parent_external_activity_id: Option<ExternalId>,
    pub(crate) parent_external_item_id: ExternalId,
    pub(crate) activity_kind: RequiredText<128>,
    pub(crate) role: ActivityRole,
    pub(crate) activity_lifecycle: ActivityLifecycle,
    pub(crate) depth: u8,
    pub(crate) observed_sequence: u64,
    pub(crate) provenance: ActivityProvenanceV1,
    pub(crate) item_lifecycle: ItemLifecycle,
    pub(crate) item_tool_state: Option<ToolState>,
    pub(crate) payload: NormalizedPayload,
    pub(crate) payload_digest: String,
    pub(crate) now_millis: i64,
}
