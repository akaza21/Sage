use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    intent::{
        ActivityProvenanceV1, CapabilitySnapshotV1, ExternalId, ModelId, NormalizedPayload,
        PageCursor, Prompt, SafeResourceRef, UnavailableResourceReason,
    },
    lifecycle::{ActivityLifecycle, ItemLifecycle, ToolState, TurnState, TurnTerminalReason},
    limits::LIMITS,
    projection::ActivityRole,
    transport::{Frame, OutboundFrame},
};

mod egress_resolution;
mod invalid_response;
pub(crate) use invalid_response::response_id_for_discard;
mod model;
pub(crate) use model::{
    AdapterIdentity, AdapterModel, AdapterOptionalEgress, AdapterRuntimeError, HarnessAdapter,
};

/// Native-only command input. This type deliberately has no `Debug` or
/// serialization implementation because it can contain a canonical absolute
/// path or prompt text.
pub(crate) enum AdapterCommand {
    Initialize,
    ModelPage {
        cursor: Option<PageCursor>,
    },
    NewSession {
        native_root: String,
        model: Option<ModelId>,
    },
    ResumeSession {
        external_session_id: ExternalId,
        native_root: String,
        model: Option<ModelId>,
    },
    LoadSession {
        external_session_id: ExternalId,
    },
    CloseSession,
    Prompt {
        prompt: Prompt,
        client_message_id: RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
    },
    Steer {
        prompt: Prompt,
        client_message_id: RequiredText<512>,
    },
    Cancel,
    ResolvePermission {
        request_id: SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
    },
}

pub(crate) trait ResourceMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason>;

    fn map_missing_display(&self, _absolute_path: &str) -> Option<SafeResourceRef> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterActivityItem {
    pub(crate) activity_id: AdapterIdentity,
    pub(crate) item_id: AdapterIdentity,
    pub(crate) parent_activity_id: Option<AdapterIdentity>,
    pub(crate) parent_item_id: AdapterIdentity,
    pub(crate) activity_kind: RequiredText<128>,
    pub(crate) role: ActivityRole,
    pub(crate) activity_lifecycle: ActivityLifecycle,
    pub(crate) depth: u8,
    pub(crate) observed_sequence: u64,
    pub(crate) provenance: ActivityProvenanceV1,
    pub(crate) item_lifecycle: ItemLifecycle,
    pub(crate) item_tool_state: Option<ToolState>,
    pub(crate) payload: NormalizedPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum ItemUpdateMode {
    Append,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) enum AdapterEvent {
    Initialized {
        capabilities: CapabilitySnapshotV1,
    },
    SessionReady {
        id: AdapterIdentity,
        loaded: bool,
    },
    Models {
        models: Vec<AdapterModel>,
        next_cursor: Option<PageCursor>,
    },
    Turn {
        id: AdapterIdentity,
        state: TurnState,
    },
    TerminalReason {
        id: AdapterIdentity,
        reason: TurnTerminalReason,
        message: Option<crate::acp::contract::SafeText<8192>>,
    },
    Item {
        id: AdapterIdentity,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    },
    ChildThreadItem {
        id: AdapterIdentity,
        lifecycle: ItemLifecycle,
        payload: NormalizedPayload,
        child_thread_id: ExternalId,
    },
    ChildLiveItem {
        child_thread_id: ExternalId,
        child_turn_id: ExternalId,
        item_id: ExternalId,
        role: Option<crate::acp::bindings::codex::ChildTextRole>,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    },
    ChildTurnStatus {
        child_thread_id: ExternalId,
        child_turn_id: ExternalId,
        status: TurnState,
    },
    FilePair {
        id: AdapterIdentity,
        external_item_id: ExternalId,
        old_text: String,
        new_text: String,
    },
    ActivityItem(Box<AdapterActivityItem>),
    Usage {
        turn_id: AdapterIdentity,
        payload: NormalizedPayload,
    },
    PermissionRequest {
        turn_id: AdapterIdentity,
        item_id: AdapterIdentity,
        external_request_id: ExternalId,
        payload: NormalizedPayload,
        request_payload_digest: [u8; 32],
    },
    PermissionResolved {
        turn_id: AdapterIdentity,
        external_request_id: ExternalId,
        state: crate::acp::lifecycle::ServerRequestState,
    },
    Acknowledged,
    SteerResponse {
        request_id: u64,
        accepted: bool,
    },
    Absorbed,
    Unsupported(NormalizedPayload),
    Error(NormalizedPayload),
    IgnoredLate,
    IgnoredDuplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EgressToken(EgressIntent);

#[derive(Debug, Clone, PartialEq, Eq)]
enum EgressIntent {
    Request(u64),
    ProtocolNotification,
    CancelRequest(u64),
    CancelNotification,
    MandatoryDeny {
        request_id: ExternalId,
        replay: bool,
    },
    PermissionCancel {
        request_id: ExternalId,
    },
    PermissionAllow {
        request_id: ExternalId,
    },
    ClientFsResponse,
}

impl EgressToken {
    pub(crate) fn request(id: u64) -> Self {
        Self(EgressIntent::Request(id))
    }

    pub(crate) fn protocol_notification() -> Self {
        Self(EgressIntent::ProtocolNotification)
    }

    pub(crate) fn cancel_request(id: u64) -> Self {
        Self(EgressIntent::CancelRequest(id))
    }

    pub(crate) fn cancel_notification() -> Self {
        Self(EgressIntent::CancelNotification)
    }

    pub(crate) fn mandatory_deny(request_id: ExternalId, replay: bool) -> Self {
        Self(EgressIntent::MandatoryDeny { request_id, replay })
    }

    pub(crate) fn permission_allow(request_id: ExternalId) -> Self {
        Self(EgressIntent::PermissionAllow { request_id })
    }

    pub(crate) fn permission_cancel(request_id: ExternalId) -> Self {
        Self(EgressIntent::PermissionCancel { request_id })
    }

    pub(crate) fn client_fs_response() -> Self {
        Self(EgressIntent::ClientFsResponse)
    }

    pub(crate) fn is_client_fs_response(&self) -> bool {
        matches!(self.0, EgressIntent::ClientFsResponse)
    }

    pub(crate) fn is_mandatory(&self) -> bool {
        self.deny_request().is_some() || self.cancelled_permission_request().is_some()
    }

    pub(crate) fn is_control(&self) -> bool {
        matches!(
            self.0,
            EgressIntent::CancelRequest(_)
                | EgressIntent::CancelNotification
                | EgressIntent::MandatoryDeny { .. }
                | EgressIntent::PermissionCancel { .. }
                | EgressIntent::PermissionAllow { .. }
                | EgressIntent::ClientFsResponse
        )
    }

    pub(crate) fn request_id(&self) -> Option<u64> {
        match self.0 {
            EgressIntent::Request(id) | EgressIntent::CancelRequest(id) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn deny_request(&self) -> Option<(&str, bool)> {
        match &self.0 {
            EgressIntent::MandatoryDeny { request_id, replay } => {
                Some((request_id.as_str(), *replay))
            }
            _ => None,
        }
    }

    pub(crate) fn allow_request(&self) -> Option<&str> {
        match &self.0 {
            EgressIntent::PermissionAllow { request_id } => Some(request_id.as_str()),
            _ => None,
        }
    }

    pub(crate) fn cancelled_permission_request(&self) -> Option<&str> {
        match &self.0 {
            EgressIntent::PermissionCancel { request_id } => Some(request_id.as_str()),
            _ => None,
        }
    }

    pub(crate) fn is_notification(&self) -> bool {
        matches!(
            self.0,
            EgressIntent::ProtocolNotification | EgressIntent::CancelNotification
        )
    }

    pub(crate) fn is_cancel(&self) -> bool {
        self.is_control() && (self.request_id().is_some() || self.is_notification())
    }
}

pub(crate) struct AdapterEgress {
    frame: OutboundFrame,
    token: EgressToken,
}

impl AdapterEgress {
    pub(crate) fn new(frame: OutboundFrame, token: EgressToken) -> Self {
        Self { frame, token }
    }

    pub(crate) fn frame(&self) -> &OutboundFrame {
        &self.frame
    }

    pub(crate) fn token(&self) -> &EgressToken {
        &self.token
    }

    pub(crate) fn into_frame(self) -> OutboundFrame {
        self.frame
    }

    pub(crate) fn into_token(self) -> EgressToken {
        self.token
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EgressResolution {
    QueueRejected,
    Queued,
    WriteZeroBytes,
    Written,
    DeliveryUnknown,
}

pub(crate) struct AdapterBatch {
    events: Vec<AdapterEvent>,
    egress: Vec<AdapterEgress>,
    actual_bytes: usize,
}

impl AdapterBatch {
    pub(crate) fn new(
        events: Vec<AdapterEvent>,
        egress: Vec<AdapterEgress>,
    ) -> Result<Self, AdapterRuntimeError> {
        let mut builder = AdapterBatchBuilder::new();
        for event in events {
            builder.push_event(event)?;
        }
        for value in egress {
            builder.push_egress(value)?;
        }
        Ok(builder.finish())
    }

    pub(crate) fn events(&self) -> &[AdapterEvent] {
        &self.events
    }

    pub(crate) fn egress(&self) -> &[AdapterEgress] {
        &self.egress
    }

    pub(crate) fn actual_bytes(&self) -> usize {
        self.actual_bytes
    }

    pub(crate) fn into_parts(self) -> (Vec<AdapterEvent>, Vec<AdapterEgress>) {
        (self.events, self.egress)
    }
}

pub(crate) struct AdapterBatchBuilder {
    events: Vec<AdapterEvent>,
    egress: Vec<AdapterEgress>,
    event_bytes: usize,
    egress_bytes: usize,
}

mod batch_builder;

mod refusal_detail;
pub(crate) use refusal_detail::RefusalDetail;

pub(crate) const PROVISIONAL_TURN_HANDOFF_MILLIS: u64 = 10_000;

mod byte_count;
use byte_count::event_bytes;
