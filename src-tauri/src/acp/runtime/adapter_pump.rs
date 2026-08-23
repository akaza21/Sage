use std::sync::Arc;

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressResolution, HarnessAdapter, ResourceMapper,
    },
    contract::{
        ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId, SageServerRequestId, SageTurnId,
    },
    intent::{ExternalId, NormalizedPayload},
    lifecycle::{ItemLifecycle, ServerRequestState, ToolState, TurnState},
    limits::LIMITS,
    runtime::{ProjectionPublishStatus, ProjectionPublisher, permissions::PermissionIntent},
    store::AdapterTurnCursor,
    transport::{
        Frame, FrameWriteOutcome, IncrementalFrameDecoder, OutboundFrame, ReservedQueue,
        validate_structure,
    },
};

mod activity;
mod batch_journal;
mod coalescer;
mod commit;
mod completion;
mod egress;
mod ingress;
mod normalize;
mod payload;
mod receipt;

pub(crate) use activity::DurableActivityItem;
pub(crate) use batch_journal::{AdapterBatchJournal, JournalReceipt, PumpJournalError};
pub(crate) use coalescer::DeferredIngressState;
use coalescer::PendingIngress;
#[allow(unused_imports)]
pub(crate) use completion::CompletedPumpParts;
pub(crate) use payload::normalized_payload_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveTurnIntent {
    pub turn_id: SageTurnId,
    pub external_turn_id: Option<ExternalId>,
    pub client_message_id: RequiredText<512>,
}

impl ActiveTurnIntent {
    /// Constructs the first adapter correlation from the sole authoritative
    /// Sage Turn ID. The adapter may latch its opaque external Turn ID only
    /// after a correlated ingress event.
    pub(crate) fn store_minted(
        turn_id: SageTurnId,
    ) -> Result<Self, crate::acp::contract::ContractError> {
        let client_message_id = RequiredText::new("clientMessageId", turn_id.as_str().to_owned())?;
        Ok(Self {
            turn_id,
            external_turn_id: None,
            client_message_id,
        })
    }
}

/// Output of the separately owned activation/negotiation coordinator. The
/// turn-stream pump is created only after this durable session tuple exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionPumpIntent {
    pub binding_digest: [u8; 32],
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub external_session_id: ExternalId,
    pub runtime_epoch: RuntimeEpoch,
}

impl SessionPumpIntent {
    pub(crate) fn bind_turn(self, active_turn: ActiveTurnIntent) -> PumpIntent {
        PumpIntent {
            binding_digest: self.binding_digest,
            project_id: self.project_id,
            chat_id: self.chat_id,
            session_id: self.session_id,
            external_session_id: self.external_session_id,
            runtime_epoch: self.runtime_epoch,
            active_turn,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PumpIntent {
    pub binding_digest: [u8; 32],
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub external_session_id: ExternalId,
    pub runtime_epoch: RuntimeEpoch,
    pub active_turn: ActiveTurnIntent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnScope {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub binding_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DurableAdapterEvent {
    Turn {
        scope: TurnScope,
        state: TurnState,
        now_millis: i64,
    },
    Item {
        scope: TurnScope,
        external_item_id: ExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
        payload_digest: String,
        now_millis: i64,
    },
    ActivityItem(Box<DurableActivityItem>),
    PermissionRequest {
        intent: PermissionIntent,
        now_millis: i64,
    },
    /// Legacy paired-denial input retained for recovery characterization.
    /// New ingress normalizes to PermissionRequest and waits for a decision.
    PermissionDeny {
        intent: PermissionIntent,
        now_millis: i64,
    },
    RuntimePayload {
        scope: TurnScope,
        external_item_id: ExternalId,
        payload: NormalizedPayload,
        payload_digest: String,
        now_millis: i64,
    },
}

pub(crate) trait EgressTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueDisposition {
    Queued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushDisposition {
    Idle,
    Written,
    ZeroBytes,
    DeliveryUnknown,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AdapterPumpError {
    #[error("adapter event or command has stale or foreign intent")]
    Intent,
    #[error("there is no exact active Turn for cancellation")]
    NoActiveTurn,
    #[error("adapter runtime is tearing down fail closed")]
    TeardownRequired,
    #[error("adapter batch exceeds a frozen count or actual-byte limit")]
    BatchCapacity,
    #[error("adapter egress is backpressured at its bounded capacity")]
    Backpressure,
    #[error("durable adapter journal failed")]
    Journal(#[from] PumpJournalError),
    #[error("projection ownership changed; authoritative bootstrap is required")]
    BootstrapRequired,
    #[error("projection delivery failed; authoritative recovery is required")]
    RecoveryRequired,
    #[error("adapter frame is malformed")]
    Frame,
    #[error(transparent)]
    Adapter(#[from] AdapterRuntimeError),
}

pub(crate) struct AdapterPump<A, J, R> {
    adapter: A,
    journal: J,
    resources: R,
    publisher: Arc<dyn ProjectionPublisher>,
    intent: PumpIntent,
    decoder: IncrementalFrameDecoder,
    decoder_frame_aligned: bool,
    egress: ReservedQueue<AdapterEgress>,
    pending: PendingIngress,
    requests: Vec<(ExternalId, SageServerRequestId)>,
    next_synthetic_item: u64,
    terminal: Option<TurnState>,
    durable_turn_cursor: Option<AdapterTurnCursor>,
    teardown: bool,
    projection_recovery_required: bool,
    pending_permission: Option<PendingPermission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingPermission {
    intent: PermissionIntent,
    written: bool,
    decision: Option<crate::acp::runtime::permissions::PermissionDecision>,
}

pub(crate) struct AdapterPumpAdmission {
    egress: ReservedQueue<AdapterEgress>,
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) fn new(
        adapter: A,
        journal: J,
        resources: R,
        publisher: Arc<dyn ProjectionPublisher>,
        intent: PumpIntent,
    ) -> Result<Self, AdapterPumpError> {
        let admission = Self::admit(&intent)?;
        Ok(Self::from_admission(
            adapter, journal, resources, publisher, intent, admission,
        ))
    }

    pub(crate) fn admit(intent: &PumpIntent) -> Result<AdapterPumpAdmission, AdapterPumpError> {
        if intent.runtime_epoch == RuntimeEpoch::ZERO || intent.binding_digest == [0; 32] {
            return Err(AdapterPumpError::Intent);
        }
        Ok(AdapterPumpAdmission { egress: queue()? })
    }

    pub(crate) fn from_admission(
        adapter: A,
        journal: J,
        resources: R,
        publisher: Arc<dyn ProjectionPublisher>,
        intent: PumpIntent,
        admission: AdapterPumpAdmission,
    ) -> Self {
        Self {
            adapter,
            journal,
            resources,
            publisher,
            intent,
            decoder: IncrementalFrameDecoder::new(),
            decoder_frame_aligned: true,
            egress: admission.egress,
            pending: PendingIngress::new(),
            requests: Vec::new(),
            next_synthetic_item: 0,
            terminal: None,
            durable_turn_cursor: None,
            teardown: false,
            projection_recovery_required: false,
            pending_permission: None,
        }
    }

    fn with_queue(
        adapter: A,
        journal: J,
        resources: R,
        publisher: Arc<dyn ProjectionPublisher>,
        intent: PumpIntent,
        egress: ReservedQueue<AdapterEgress>,
    ) -> Result<Self, AdapterPumpError> {
        if intent.runtime_epoch == RuntimeEpoch::ZERO || intent.binding_digest == [0; 32] {
            return Err(AdapterPumpError::Intent);
        }
        Ok(Self::from_admission(
            adapter,
            journal,
            resources,
            publisher,
            intent,
            AdapterPumpAdmission { egress },
        ))
    }

    pub(crate) fn teardown_required(&self) -> bool {
        self.teardown
    }

    /// Exposes only the store-minted durable cursor, including after a
    /// post-commit projection publication failure. The host can therefore
    /// release the exact Turn lease before it tears the process tree down.
    pub(crate) fn latest_durable_turn_cursor(&self) -> Option<&AdapterTurnCursor> {
        self.durable_turn_cursor.as_ref()
    }

    pub(crate) fn deferred_ingress_state(&self, now_millis: i64) -> DeferredIngressState {
        self.pending.state(now_millis)
    }

    pub(crate) fn next_deferred_flush_deadline_millis(&self) -> Option<i64> {
        self.pending.deadline_millis()
    }

    fn request_id(
        &mut self,
        external: &ExternalId,
    ) -> Result<SageServerRequestId, AdapterPumpError> {
        if let Some((_, id)) = self.requests.iter().find(|(value, _)| value == external) {
            return Ok(id.clone());
        }
        // This identity map preserves duplicate correlation for durable history;
        // the SQLite authority separately enforces the smaller unresolved-live cap.
        if self.requests.len() >= LIMITS.durable.requests_per_session as usize {
            self.teardown = true;
            return Err(AdapterPumpError::BatchCapacity);
        }
        self.requests
            .try_reserve_exact(1)
            .map_err(|_| AdapterPumpError::BatchCapacity)?;
        let id = SageServerRequestId::new();
        self.requests.push((external.clone(), id.clone()));
        Ok(id)
    }

    fn synthetic_item_id(&mut self) -> Result<ExternalId, AdapterPumpError> {
        self.next_synthetic_item = self
            .next_synthetic_item
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(AdapterPumpError::BatchCapacity)?;
        ExternalId::new(
            "runtimeItemId",
            format!(
                "sage-runtime-{}-{}",
                self.intent.runtime_epoch.get(),
                self.next_synthetic_item
            ),
        )
        .map_err(|_| AdapterPumpError::BatchCapacity)
    }

    fn exact_session(&self, id: &crate::acp::bindings::adapter::AdapterIdentity) -> bool {
        id.binding_digest == self.intent.binding_digest
            && id.runtime_epoch == self.intent.runtime_epoch
            && id.session_id == self.intent.session_id
            && id.external_id == self.intent.external_session_id
            && id.turn_id.is_none()
            && id.turn_client_message_id.is_none()
    }

    fn exact_turn(&mut self, id: &crate::acp::bindings::adapter::AdapterIdentity) -> bool {
        let exact_scope = id.binding_digest == self.intent.binding_digest
            && id.runtime_epoch == self.intent.runtime_epoch
            && id.session_id == self.intent.session_id
            && id.turn_client_message_id.as_ref()
                == Some(&self.intent.active_turn.client_message_id);
        if !exact_scope {
            return false;
        }
        let Some(external_turn_id) = id.turn_id.as_ref() else {
            return false;
        };
        match self.intent.active_turn.external_turn_id.as_ref() {
            Some(expected) => expected == external_turn_id,
            None => {
                self.intent.active_turn.external_turn_id = Some(external_turn_id.clone());
                true
            }
        }
    }

    fn scope(&self) -> TurnScope {
        TurnScope {
            project_id: self.intent.project_id.clone(),
            chat_id: self.intent.chat_id.clone(),
            session_id: self.intent.session_id.clone(),
            runtime_epoch: self.intent.runtime_epoch,
            turn_id: self.intent.active_turn.turn_id.clone(),
            binding_digest: hex(&self.intent.binding_digest),
        }
    }

    fn ensure_live(&self) -> Result<(), AdapterPumpError> {
        if self.teardown
            || self
                .pending_permission
                .as_ref()
                .is_some_and(|permission| permission.written)
        {
            Err(AdapterPumpError::TeardownRequired)
        } else {
            Ok(())
        }
    }
}

fn queue() -> Result<ReservedQueue<AdapterEgress>, AdapterPumpError> {
    ReservedQueue::new(
        LIMITS.queues.egress_frames as usize,
        LIMITS.queues.egress_bytes as usize,
        LIMITS.queues.egress_control_frames_inside_total as usize,
        LIMITS.queues.egress_control_bytes_inside_total as usize,
    )
    .map_err(|_| AdapterPumpError::Backpressure)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "adapter_pump/tests.rs"]
mod tests;
