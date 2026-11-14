use std::{future::Future, pin::Pin};

use crate::acp::{
    bindings::adapter::{
        AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity, AdapterModel,
        AdapterRuntimeError, EgressResolution, HarnessAdapter, ResourceMapper,
    },
    store::CompleteActivationCommand,
    transport::{FrameWriteOutcome, IncrementalFrameDecoder, ReservedQueue, validate_structure},
};
use sage_acp_protocol::scheduler::RuntimeOwner;
use sage_acp_protocol::{
    contract::{
        ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId,
    },
    intent::{
        CapabilitySnapshotV1, ExternalId, PageCursor,
        session_event::{PendingSessionEvents, SessionScopedEvent},
    },
};
use sage_core::limits::LIMITS;

mod capabilities;
mod coordinator_journal;
mod ingress;
mod intent;
mod mode;
mod parts;

use intent::{digest, validate_intent};

pub(crate) use coordinator_journal::{
    CoordinatorActivationHandoff, CoordinatorActivationJournal, CoordinatorShutdownHandoff,
};
pub(crate) use ingress::is_available_commands_update;
pub(crate) use mode::ActivationMode;
pub(crate) use parts::{IdleActivationParts, RecoveryActivationParts, TeardownActivationParts};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct ActivationIntent {
    pub binding_digest: [u8; 32],
    pub artifact_digest: String,
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub mode: ActivationMode,
    pub curated_ceiling: CapabilitySnapshotV1,
    pub sage_policy: CapabilitySnapshotV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivatedSession {
    pub binding_digest: [u8; 32],
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub external_session_id: ExternalId,
    pub runtime_epoch: RuntimeEpoch,
    pub capabilities: CapabilitySnapshotV1,
    pub models: Vec<AdapterModel>,
    pub next_model_cursor: Option<PageCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActivationReceipt {
    pub projection_revision: ProjectionRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ActivationJournalError {
    #[error("activation was rejected before commit")]
    Rejected,
    #[error("activation commit outcome is unknown")]
    CommitUnknown,
    #[error("activation committed but projection ownership requires bootstrap")]
    BootstrapRequired,
    #[error("activation committed but projection delivery requires recovery")]
    RecoveryRequired,
}

pub(crate) trait ActivationJournal: Send {
    fn commit_ready<'a>(
        &'a mut self,
        command: CompleteActivationCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ActivationReceipt, ActivationJournalError>> + Send + 'a>>;
}

pub(crate) trait ActivationTransport {
    fn write(&mut self, frame: &crate::acp::transport::OutboundFrame) -> FrameWriteOutcome;
}

impl ActivationTransport for crate::acp::process::StdinEndpoint {
    fn write(&mut self, frame: &crate::acp::transport::OutboundFrame) -> FrameWriteOutcome {
        self.write_frame(frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationFlush {
    Idle,
    Written,
    RetriedZeroBytes,
    DeliveryUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Dormant,
    InitializeQueued,
    AwaitingInitialized,
    Initialized,
    SessionQueued,
    AwaitingSession,
    Ready,
    RecoveryRequired,
    Teardown,
}

impl Phase {
    /// Adapter requests that join the bounded initialization budget: the
    /// handshake itself, and ACP sign-in while the session is not yet ready
    /// (`authenticate`, then `session/new` again).
    fn admits_initialization_request(self, session_ready: bool) -> bool {
        self == Self::AwaitingInitialized || (self == Self::AwaitingSession && !session_ready)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ActivationPumpError {
    #[error("activation intent or adapter completion is stale or foreign")]
    Intent,
    #[error("activation refused the advertised protocol version")]
    RefusedVersion(RequiredText<64>),
    #[error("activation metadata exceeds a frozen count or actual-byte limit")]
    Capacity,
    #[error("activation command is backpressured at its bounded capacity")]
    Backpressure,
    #[error("activation stdout is malformed")]
    Frame,
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
    #[error("activation delivery may have written bytes and cannot be replayed")]
    DeliveryUnknown,
    #[error("activation zero-byte retry is exhausted")]
    RetryExhausted,
    #[error("activation is blocked pending authoritative projection bootstrap")]
    BootstrapRequired,
    #[error("activation is blocked pending authoritative projection recovery")]
    RecoveryRequired,
    #[error("activation ownership is not yet quiescent for Turn handoff")]
    NotQuiescent,
    #[error("activation runtime must be torn down fail closed")]
    TeardownRequired,
    #[error(transparent)]
    Adapter(#[from] AdapterRuntimeError),
    #[error(transparent)]
    Journal(#[from] ActivationJournalError),
}

pub(crate) struct ActivationPump<A, J, R> {
    adapter: A,
    journal: J,
    resources: R,
    intent: ActivationIntent,
    decoder: IncrementalFrameDecoder,
    decoder_frame_aligned: bool,
    egress: ReservedQueue<AdapterEgress>,
    phase: Phase,
    initialize_attempts: u8,
    initialization_requests: u64,
    session_attempts: u8,
    capabilities: Option<CapabilitySnapshotV1>,
    models: Vec<AdapterModel>,
    next_model_cursor: Option<PageCursor>,
    ready: Option<ActivatedSession>,
    /// Session events seen before the harness session identity is known.
    pending_session: PendingSessionEvents<(AdapterIdentity, SessionScopedEvent)>,
}

type ActivationHandoff<T, A, J, R> = Result<T, Box<(ActivationPump<A, J, R>, ActivationPumpError)>>;
pub(crate) type RetainedActivationConstruction<A, J, R> =
    Result<ActivationPump<A, J, R>, Box<(A, J, R, ActivationPumpError)>>;

impl<A: HarnessAdapter, J: ActivationJournal, R: ResourceMapper> ActivationPump<A, J, R> {
    #[cfg(any(test, all(windows, feature = "acp-fixture")))]
    pub(crate) fn new(
        adapter: A,
        journal: J,
        resources: R,
        intent: ActivationIntent,
    ) -> Result<Self, ActivationPumpError> {
        Self::new_retained(adapter, journal, resources, intent).map_err(|error| error.3)
    }

    pub(crate) fn new_retained(
        adapter: A,
        journal: J,
        resources: R,
        intent: ActivationIntent,
    ) -> RetainedActivationConstruction<A, J, R> {
        if let Err(error) = validate_intent(&intent) {
            return Err(Box::new((adapter, journal, resources, error)));
        }
        let egress = match ReservedQueue::new(
            LIMITS.queues.egress_frames as usize,
            LIMITS.queues.egress_bytes as usize,
            LIMITS.queues.egress_control_frames_inside_total as usize,
            LIMITS.queues.egress_control_bytes_inside_total as usize,
        ) {
            Ok(egress) => egress,
            Err(_) => {
                return Err(Box::new((
                    adapter,
                    journal,
                    resources,
                    ActivationPumpError::Backpressure,
                )));
            }
        };
        Ok(Self {
            adapter,
            journal,
            resources,
            intent,
            decoder: IncrementalFrameDecoder::new(),
            decoder_frame_aligned: true,
            egress,
            phase: Phase::Dormant,
            initialize_attempts: 0,
            initialization_requests: 0,
            session_attempts: 0,
            capabilities: None,
            models: Vec::new(),
            next_model_cursor: None,
            ready: None,
            pending_session: PendingSessionEvents::default(),
        })
    }

    pub(crate) fn begin(&mut self) -> Result<(), ActivationPumpError> {
        if self.phase != Phase::Dormant {
            return Err(ActivationPumpError::Intent);
        }
        self.queue_initialize()
    }

    pub(crate) fn runtime_owner(&self) -> RuntimeOwner {
        RuntimeOwner {
            project_id: self.intent.project_id.clone(),
            chat_id: self.intent.chat_id.clone(),
            session_id: self.intent.session_id.clone(),
            runtime_epoch: self.intent.runtime_epoch,
        }
    }

    pub(crate) fn force_teardown(&mut self) {
        self.fail_closed();
    }

    pub(crate) fn teardown_required(&self) -> bool {
        self.phase == Phase::Teardown
    }

    #[cfg(test)]
    pub(crate) fn projection_recovery_required(&self) -> bool {
        self.phase == Phase::RecoveryRequired
    }

    pub(crate) fn completion_ready(&self) -> bool {
        self.phase == Phase::Ready
            && self.ready.is_some()
            && self.decoder_frame_aligned
            && !self.decoder.has_pending_input()
            && self.egress.data_usage().0 == 0
            && self.egress.control_usage().0 == 0
    }

    pub(crate) fn startup_phase(&self) -> &'static str {
        match self.phase {
            Phase::Dormant | Phase::InitializeQueued | Phase::AwaitingInitialized => "initialize",
            Phase::Initialized | Phase::SessionQueued | Phase::AwaitingSession => {
                match self.intent.mode {
                    ActivationMode::New { .. } => "session/new",
                    ActivationMode::Resume { .. } => "session/resume",
                    ActivationMode::Load { .. } => "session/load",
                }
            }
            Phase::Ready | Phase::RecoveryRequired | Phase::Teardown => "ready-handoff",
        }
    }

    pub(crate) fn try_into_idle_parts(
        mut self,
    ) -> ActivationHandoff<IdleActivationParts<A, J, R>, A, J, R> {
        if !self.completion_ready() {
            return Err(Box::new((self, ActivationPumpError::NotQuiescent)));
        }
        let session = self.ready.take().expect("completion readiness checked");
        let mut pending = PendingSessionEvents::default();
        for (id, event) in self.pending_session.take() {
            if id.is_exact_session(
                &session.binding_digest,
                session.runtime_epoch,
                &session.session_id,
                &session.external_session_id,
            ) {
                pending.push(event);
            }
        }
        Ok(
            IdleActivationParts::new(self.adapter, self.journal, self.resources, session)
                .with_pending_session_events(pending),
        )
    }

    pub(crate) fn try_into_recovery_parts(
        self,
    ) -> ActivationHandoff<RecoveryActivationParts<A, J, R>, A, J, R> {
        if self.phase != Phase::RecoveryRequired {
            return Err(Box::new((self, ActivationPumpError::NotQuiescent)));
        }
        Ok(RecoveryActivationParts {
            adapter: self.adapter,
            journal: self.journal,
            resources: self.resources,
        })
    }

    pub(crate) fn try_into_teardown_parts(
        self,
    ) -> ActivationHandoff<TeardownActivationParts<A, J, R>, A, J, R> {
        if self.phase != Phase::Teardown {
            return Err(Box::new((self, ActivationPumpError::NotQuiescent)));
        }
        Ok(TeardownActivationParts {
            adapter: self.adapter,
            journal: self.journal,
            resources: self.resources,
        })
    }

    fn ensure_live(&self) -> Result<(), ActivationPumpError> {
        match self.phase {
            Phase::Teardown => Err(ActivationPumpError::TeardownRequired),
            Phase::RecoveryRequired => Err(ActivationPumpError::RecoveryRequired),
            _ => Ok(()),
        }
    }

    fn queue_initialize(&mut self) -> Result<(), ActivationPumpError> {
        self.initialize_attempts = self
            .initialize_attempts
            .checked_add(1)
            .ok_or(ActivationPumpError::RetryExhausted)?;
        let value = self.adapter.command(AdapterCommand::Initialize)?;
        self.enqueue_initialization(value)?;
        self.phase = Phase::InitializeQueued;
        Ok(())
    }

    fn queue_session(&mut self) -> Result<(), ActivationPumpError> {
        self.session_attempts = self
            .session_attempts
            .checked_add(1)
            .ok_or(ActivationPumpError::RetryExhausted)?;
        let command = match &self.intent.mode {
            ActivationMode::New { native_root, model } => AdapterCommand::NewSession {
                native_root: native_root.clone(),
                model: model.clone(),
            },
            ActivationMode::Resume {
                external_session_id,
                native_root,
                model,
                ..
            } => AdapterCommand::ResumeSession {
                external_session_id: external_session_id.clone(),
                native_root: native_root.clone(),
                model: model.clone(),
            },
            ActivationMode::Load {
                external_session_id,
            } => AdapterCommand::LoadSession {
                external_session_id: external_session_id.clone(),
            },
        };
        let value = self.adapter.command(command)?;
        self.enqueue(value)?;
        self.phase = Phase::SessionQueued;
        Ok(())
    }

    fn enqueue(&mut self, value: AdapterEgress) -> Result<(), ActivationPumpError> {
        let token = value.token().clone();
        if token.is_control() || token.is_mandatory() {
            self.fail_closed();
            return Err(ActivationPumpError::Intent);
        }
        let bytes = value.frame().wire_len();
        let result = self.egress.try_push_data_with(bytes, || value);
        if result.is_err() {
            let _ = self
                .adapter
                .resolve_egress(token, EgressResolution::QueueRejected);
            self.fail_closed();
            return Err(ActivationPumpError::Backpressure);
        }
        if self
            .adapter
            .resolve_egress(token, EgressResolution::Queued)
            .is_err()
        {
            self.fail_closed();
            return Err(ActivationPumpError::TeardownRequired);
        }
        Ok(())
    }

    fn fail_closed(&mut self) {
        self.phase = Phase::Teardown;
        self.ready = None;
    }
}
