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
    lifecycle::{ItemLifecycle, ServerRequestState, ToolState, TurnState, TurnTerminalReason},
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
mod cancel;
mod coalescer;
mod commit;
mod completion;
mod egress;
use egress::queue;
mod error;
mod events;
pub(crate) use events::{DurableAdapterEvent, TurnScope};
mod ingress;
mod normalize;
mod payload;
mod provisional;
mod receipt;
pub(crate) mod session_config;

pub(crate) use activity::DurableActivityItem;
pub(crate) use batch_journal::{AdapterBatchJournal, JournalReceipt, PumpJournalError};
pub(crate) use coalescer::DeferredIngressState;
use coalescer::PendingIngress;
#[allow(unused_imports)]
pub(crate) use completion::CompletedPumpParts;
pub(crate) use error::{AdapterPumpError, NonfatalRefusal};
use payload::hex;
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

pub(crate) struct AdapterPump<A, J, R> {
    adapter: A,
    journal: J,
    resources: R,
    publisher: Arc<dyn ProjectionPublisher>,
    intent: PumpIntent,
    decoder: IncrementalFrameDecoder,
    decoder_frame_aligned: bool,
    oversize_event: bool,
    stdout_preamble: Option<StdoutPreambleWindow>,
    egress: ReservedQueue<AdapterEgress>,
    pending: PendingIngress,
    provisional_turn_deadline_millis: Option<i64>,
    next_synthetic_item: u64,
    terminal: Option<TurnState>,
    durable_turn_cursor: Option<AdapterTurnCursor>,
    teardown: bool,
    projection_recovery_required: bool,
    pending_permissions: Vec<PendingPermission>,
    pending_steer_request: Option<u64>,
    steer_response: Option<bool>,
}

const STDOUT_PREAMBLE_MAX_LINES: u32 = 16;
const STDOUT_PREAMBLE_MAX_BYTES: usize = 8_192;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StdoutPreambleWindow {
    lines: u32,
    bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingPermission {
    intent: PermissionIntent,
    written: bool,
    decision: Option<crate::acp::runtime::permissions::PermissionDecision>,
}

pub(crate) struct AdapterPumpAdmission {
    egress: ReservedQueue<AdapterEgress>,
    pending_permissions: Vec<PendingPermission>,
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
        Ok(AdapterPumpAdmission {
            egress: queue()?,
            pending_permissions: pending_permission_slots()?,
        })
    }

    pub(crate) fn from_admission(
        mut adapter: A,
        journal: J,
        resources: R,
        publisher: Arc<dyn ProjectionPublisher>,
        intent: PumpIntent,
        admission: AdapterPumpAdmission,
    ) -> Self {
        adapter.finish_model_config_hydration();
        Self {
            adapter,
            journal,
            resources,
            publisher,
            intent,
            decoder: IncrementalFrameDecoder::new(),
            decoder_frame_aligned: true,
            oversize_event: false,
            stdout_preamble: Some(StdoutPreambleWindow::default()),
            egress: admission.egress,
            pending: PendingIngress::new(),
            provisional_turn_deadline_millis: None,
            next_synthetic_item: 0,
            terminal: None,
            durable_turn_cursor: None,
            teardown: false,
            projection_recovery_required: false,
            pending_permissions: admission.pending_permissions,
            pending_steer_request: None,
            steer_response: None,
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
            AdapterPumpAdmission {
                egress,
                pending_permissions: pending_permission_slots()?,
            },
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
        match (
            self.pending.deadline_millis(),
            self.provisional_turn_deadline_millis,
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, None) | (None, a) => a,
        }
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
                .pending_permissions
                .iter()
                .any(|permission| permission.written)
        {
            Err(AdapterPumpError::TeardownRequired)
        } else {
            Ok(())
        }
    }
}

fn pending_permission_slots() -> Result<Vec<PendingPermission>, AdapterPumpError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(LIMITS.queues.server_requests_per_session as usize)
        .map_err(|_| AdapterPumpError::BatchCapacity)?;
    Ok(values)
}

#[cfg(test)]
#[path = "adapter_pump/tests.rs"]
mod tests;
