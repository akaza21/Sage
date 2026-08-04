use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{
        CapabilitySnapshotV1, ExternalId, ModelId, NormalizedPayload, PageCursor, Prompt,
        SafeResourceRef, UnavailableResourceReason,
    },
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
    transport::{Frame, OutboundFrame},
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
    },
    Cancel,
}

pub(crate) trait ResourceMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterIdentity {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) external_id: ExternalId,
    pub(crate) turn_id: Option<ExternalId>,
    pub(crate) turn_client_message_id: Option<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterModel {
    pub(crate) id: ModelId,
    pub(crate) name: RequiredText<512>,
    pub(crate) is_default: bool,
}

impl AdapterModel {
    pub(crate) fn actual_bytes(&self) -> Option<usize> {
        self.id.as_str().len().checked_add(self.name.as_str().len())
    }
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
    Item {
        id: AdapterIdentity,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    },
    Usage {
        turn_id: AdapterIdentity,
        payload: NormalizedPayload,
    },
    PermissionRequest {
        turn_id: AdapterIdentity,
        item_id: AdapterIdentity,
        payload: NormalizedPayload,
        request_payload_digest: [u8; 32],
    },
    Acknowledged,
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

    pub(crate) fn is_mandatory(&self) -> bool {
        matches!(self.0, EgressIntent::MandatoryDeny { .. })
    }

    pub(crate) fn is_control(&self) -> bool {
        matches!(
            self.0,
            EgressIntent::CancelRequest(_)
                | EgressIntent::CancelNotification
                | EgressIntent::MandatoryDeny { .. }
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

    pub(crate) fn is_notification(&self) -> bool {
        matches!(
            self.0,
            EgressIntent::ProtocolNotification | EgressIntent::CancelNotification
        )
    }

    pub(crate) fn is_cancel(&self) -> bool {
        matches!(
            self.0,
            EgressIntent::CancelRequest(_) | EgressIntent::CancelNotification
        )
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

impl AdapterBatchBuilder {
    pub(crate) fn new() -> Self {
        Self {
            events: Vec::new(),
            egress: Vec::new(),
            event_bytes: 0,
            egress_bytes: 0,
        }
    }

    pub(crate) fn push_event(&mut self, event: AdapterEvent) -> Result<(), AdapterRuntimeError> {
        if self.events.len() >= LIMITS.queues.journal_facts_per_transaction as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let next = self
            .event_bytes
            .checked_add(event_bytes(&event).ok_or(AdapterRuntimeError::BatchBytes)?)
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if next > LIMITS.queues.journal_bytes_per_transaction as usize {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        self.events
            .try_reserve_exact(1)
            .map_err(|_| AdapterRuntimeError::BatchBytes)?;
        self.events.push(event);
        self.event_bytes = next;
        Ok(())
    }

    pub(crate) fn push_egress(&mut self, egress: AdapterEgress) -> Result<(), AdapterRuntimeError> {
        if self.egress.len() >= LIMITS.queues.egress_control_frames_inside_total as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let next = self
            .egress_bytes
            .checked_add(egress.frame().wire_len())
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if next > LIMITS.queues.egress_control_bytes_inside_total as usize {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        self.egress
            .try_reserve_exact(1)
            .map_err(|_| AdapterRuntimeError::BatchBytes)?;
        self.egress.push(egress);
        self.egress_bytes = next;
        Ok(())
    }

    pub(crate) fn finish(self) -> AdapterBatch {
        AdapterBatch {
            events: self.events,
            egress: self.egress,
            actual_bytes: self.event_bytes + self.egress_bytes,
        }
    }
}

pub(crate) trait HarnessAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1>;
    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError>;
    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError>;
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError>;
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AdapterRuntimeError {
    #[error("adapter operation is unavailable or invalid in the current state")]
    Operation,
    #[error("the adapter does not advertise this command")]
    UnsupportedCommand,
    #[error("adapter launch artifact is not admitted")]
    LaunchUnavailable,
    #[error("adapter frame is invalid")]
    Frame,
    #[error("adapter event batch exceeds its count limit")]
    BatchCount,
    #[error("adapter event batch exceeds its actual-byte limit")]
    BatchBytes,
    #[error("mandatory deny egress failed closed and requires teardown")]
    MandatoryDenyTeardown,
    #[error("egress resolution does not match the reserved adapter intent")]
    EgressIntent,
}

fn event_bytes(event: &AdapterEvent) -> Option<usize> {
    json_bytes(event)
}

fn json_bytes(value: &impl serde::Serialize) -> Option<usize> {
    struct Counter(usize);

    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("serialized byte count overflow"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}
