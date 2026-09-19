use crate::acp::{
    contract::{ClientIntentId, SageProjectId, SageServerRequestId, SageTurnId},
    runtime::permissions::PermissionDecision,
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, PrepareTurnCommand},
};
use std::{sync::mpsc, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostState {
    Unavailable,
    Cold,
    Activating,
    Idle,
    Running,
    Draining,
    Recovery,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PromptWrite {
    Complete(SageTurnId),
    ZeroBytes(SageTurnId),
    PossibleBytes(SageTurnId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostOutcome {
    Activated,
    PromptPrepared,
    PromptAccepted,
    CancelQueued,
    PermissionResolved,
    SessionConfigured,
    TurnTerminal,
    BackgroundTurnContinues,
    ProjectSwitchObserved,
    CloseStarted,
    Closed,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum HostError {
    #[error("the bounded runtime-host queue is full")]
    Capacity,
    #[error("the runtime host is unavailable")]
    Unavailable,
    #[error("the runtime-host intent is stale or foreign")]
    Intent,
    #[error("the runtime host cannot accept this command in its current state")]
    State,
    #[error("the harness rejected the session configuration mutation")]
    ConfigMutationRejected,
    #[error("prompt delivery may have written bytes and cannot be replayed")]
    DeliveryUnknown,
    #[error("the one permitted zero-byte prompt retry is exhausted")]
    RetryExhausted,
    #[error("runtime ownership requires authoritative recovery")]
    RecoveryRequired,
    #[error("the runtime operation exceeded its bounded deadline")]
    Deadline,
    #[error("the runtime host is closed")]
    Closed,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct HostPromptIntent {
    pub(crate) command: PrepareTurnCommand,
}

impl HostPromptIntent {
    pub(crate) fn owner(&self) -> RuntimeOwner {
        RuntimeOwner {
            project_id: self.command.project_id.clone(),
            chat_id: self.command.chat_id.clone(),
            session_id: self.command.session_id.clone(),
            runtime_epoch: self.command.runtime_epoch,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct HostCancelIntent {
    pub(crate) owner: RuntimeOwner,
    pub(crate) turn_id: SageTurnId,
    pub(crate) client_intent_id: ClientIntentId,
    pub(crate) now_millis: i64,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct HostCloseIntent {
    pub(crate) command: CloseSessionCommand,
}

impl HostCloseIntent {
    pub(crate) fn owner(&self) -> RuntimeOwner {
        RuntimeOwner {
            project_id: self.command.project_id.clone(),
            chat_id: self.command.chat_id.clone(),
            session_id: self.command.session_id.clone(),
            runtime_epoch: self.command.runtime_epoch,
        }
    }
}

type ConfigCompletionCallback = Box<dyn FnOnce(Result<(), HostError>) + Send + 'static>;

pub(crate) struct ConfigCompletionSink(Option<ConfigCompletionCallback>);

impl ConfigCompletionSink {
    pub(crate) fn new(complete: impl FnOnce(Result<(), HostError>) + Send + 'static) -> Self {
        Self(Some(Box::new(complete)))
    }

    pub(crate) fn complete(mut self, result: Result<(), HostError>) {
        if let Some(complete) = self.0.take() {
            complete(result);
        }
    }
}

impl Drop for ConfigCompletionSink {
    fn drop(&mut self) {
        if let Some(complete) = self.0.take() {
            complete(Err(HostError::Closed));
        }
    }
}

pub(super) struct ConfigCompletion {
    pub(super) operation_id: u64,
    pub(super) owner: RuntimeOwner,
    pub(super) result: Result<(), HostError>,
}

pub(super) struct PendingHostConfig {
    pub(super) operation_id: u64,
    pub(super) owner: RuntimeOwner,
    pub(super) reply: mpsc::SyncSender<Result<HostReply, HostError>>,
    pub(super) deadline: Instant,
}

pub(super) struct PendingTerminal {
    pub(super) owner: RuntimeOwner,
    pub(super) cursor: AdapterTurnCursor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CancelPhase {
    Begin,
    Dispatch,
}

pub(super) struct PendingCancel {
    pub(super) intent: HostCancelIntent,
    pub(super) phase: CancelPhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ClosePhase {
    Begin,
    Finish,
}

pub(super) struct PendingClose {
    pub(super) intent: HostCloseIntent,
    pub(super) phase: ClosePhase,
}

pub(crate) trait RuntimeEngine: Send + 'static {
    fn begin_configure_session(
        &mut self,
        _intent: &crate::acp::session_config::ConfigIntent,
        _deadline: Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(Err(HostError::State));
        Ok(())
    }
    fn abandon_configuration(&mut self, _deadline: Instant) {}
    fn resolve_permission(
        &mut self,
        _owner: &RuntimeOwner,
        _request_id: &SageServerRequestId,
        _decision: PermissionDecision,
    ) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn activate(&mut self, owner: &RuntimeOwner) -> Result<(), HostError>;
    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError>;
    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError>;
    fn retry_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError>;
    fn record_delivery_unknown(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<(), HostError>;
    fn record_retry_exhausted(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<(), HostError>;
    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError>;
    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError>;
    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError>;
    fn accept_latest_completed(&mut self) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError>;
    fn converge_zero(&mut self, intent: &HostCloseIntent) -> Result<(), HostError>;
    fn finalize_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError>;
    fn begin_park(&mut self) -> Result<(), HostError> {
        Ok(())
    }
    fn park_zero(&mut self, _deadline: Instant) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, deadline: Instant) -> Result<(), HostError>;
}

/// Exact native authority retained alongside a managed runtime host. A host
/// shutdown acknowledgement alone is not zero-process evidence.
pub(crate) trait RuntimeShutdownRecovery: Send + Sync {
    fn shutdown_recovery_before(
        &self,
        deadline: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, HostError>;
}

pub(super) enum HostCommand {
    ConfigureSession(crate::acp::session_config::ConfigIntent),
    ResolvePermission {
        owner: RuntimeOwner,
        request_id: SageServerRequestId,
        decision: PermissionDecision,
    },
    Activate(RuntimeOwner),
    PreparePrompt(HostPromptIntent),
    DispatchPrompt {
        owner: RuntimeOwner,
        turn_id: SageTurnId,
    },
    Cancel(HostCancelIntent),
    Terminal {
        owner: RuntimeOwner,
        cursor: AdapterTurnCursor,
    },
    ProjectSwitched(SageProjectId),
    State,
    BeginClose(HostCloseIntent),
    FinishClose(HostCloseIntent),
    RecoverClose(HostCloseIntent),
    AcceptDurableCompleted,
    BeginPark,
    FinishPark,
    Shutdown,
}

pub(super) enum HostReply {
    Outcome(HostOutcome),
    Prepared(SageTurnId),
    State(HostState),
}

pub(crate) struct TerminalNotification {
    pub(super) response: mpsc::Receiver<Result<HostReply, HostError>>,
    pub(super) deadline: Instant,
}

impl TerminalNotification {
    pub(crate) fn wait(self) -> Result<(), HostError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        match self.response.recv_timeout(remaining) {
            Ok(Ok(HostReply::Outcome(HostOutcome::TurnTerminal))) => Ok(()),
            Ok(Ok(HostReply::Outcome(_) | HostReply::Prepared(_) | HostReply::State(_))) => {
                Err(HostError::State)
            }
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(HostError::Deadline),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
    }
}

impl HostCommand {
    pub(super) fn deadline_millis(&self) -> u64 {
        use crate::acp::limits::LIMITS;
        match self {
            Self::ConfigureSession(_) | Self::ResolvePermission { .. } => {
                LIMITS.deadlines_ms.ordinary_rpc
            }
            Self::Activate(_) => LIMITS.deadlines_ms.initialize,
            Self::Cancel(_) => LIMITS.deadlines_ms.tree_termination,
            Self::Terminal { .. }
            | Self::BeginClose(_)
            | Self::FinishClose(_)
            | Self::RecoverClose(_)
            | Self::AcceptDurableCompleted
            | Self::BeginPark
            | Self::FinishPark
            | Self::Shutdown => LIMITS.deadlines_ms.explicit_close,
            Self::PreparePrompt(_)
            | Self::DispatchPrompt { .. }
            | Self::ProjectSwitched(_)
            | Self::State => LIMITS.deadlines_ms.ordinary_rpc,
        }
    }

    pub(super) fn is_control(&self) -> bool {
        matches!(
            self,
            Self::ResolvePermission { .. }
                | Self::Cancel(_)
                | Self::Terminal { .. }
                | Self::State
                | Self::BeginClose(_)
                | Self::FinishClose(_)
                | Self::RecoverClose(_)
                | Self::AcceptDurableCompleted
                | Self::BeginPark
                | Self::FinishPark
                | Self::Shutdown
        )
    }

    pub(super) fn actual_bytes(&self) -> usize {
        fn owner_bytes(owner: &RuntimeOwner) -> usize {
            owner.project_id.as_str().len()
                + owner.chat_id.as_str().len()
                + owner.session_id.as_str().len()
                + std::mem::size_of::<u64>()
        }
        match self {
            Self::ConfigureSession(intent) => intent.actual_bytes(),
            Self::ResolvePermission {
                owner, request_id, ..
            } => owner_bytes(owner) + request_id.as_str().len() + 1,
            Self::Activate(owner) => owner_bytes(owner),
            Self::PreparePrompt(intent) => {
                owner_bytes(&intent.owner())
                    + intent.command.client_intent_id.as_str().len()
                    + intent.command.prompt.as_str().len()
                    + std::mem::size_of::<u64>() * 2
            }
            Self::DispatchPrompt { owner, turn_id } => owner_bytes(owner) + turn_id.as_str().len(),
            Self::Cancel(intent) => {
                owner_bytes(&intent.owner)
                    + intent.turn_id.as_str().len()
                    + intent.client_intent_id.as_str().len()
                    + std::mem::size_of::<i64>()
            }
            Self::BeginClose(intent) | Self::FinishClose(intent) | Self::RecoverClose(intent) => {
                owner_bytes(&intent.owner())
                    + intent.command.client_intent_id.as_str().len()
                    + std::mem::size_of::<u64>()
                    + std::mem::size_of::<i64>()
            }
            Self::Terminal { owner, cursor } => {
                owner_bytes(owner) + cursor.turn_id().as_str().len() + 32
            }
            Self::ProjectSwitched(project) => project.as_str().len(),
            Self::State
            | Self::AcceptDurableCompleted
            | Self::BeginPark
            | Self::FinishPark
            | Self::Shutdown => 1,
        }
    }
}
