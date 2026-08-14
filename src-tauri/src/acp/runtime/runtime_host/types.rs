use crate::acp::{
    contract::{ClientIntentId, SageProjectId, SageTurnId},
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, PrepareTurnCommand},
};
use std::time::Instant;

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

pub(crate) trait RuntimeEngine: Send + 'static {
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
    fn begin_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError>;
    fn converge_zero(&mut self, intent: &HostCloseIntent) -> Result<(), HostError>;
    fn finalize_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError>;
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
    Shutdown,
}

pub(super) enum HostReply {
    Outcome(HostOutcome),
    Prepared(SageTurnId),
    State(HostState),
}

impl HostCommand {
    pub(super) fn deadline_millis(&self) -> u64 {
        use crate::acp::limits::LIMITS;
        match self {
            Self::Activate(_) => LIMITS.deadlines_ms.initialize,
            Self::Cancel(_) => LIMITS.deadlines_ms.tree_termination,
            Self::Terminal { .. }
            | Self::BeginClose(_)
            | Self::FinishClose(_)
            | Self::RecoverClose(_)
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
            Self::Cancel(_)
                | Self::Terminal { .. }
                | Self::State
                | Self::BeginClose(_)
                | Self::FinishClose(_)
                | Self::RecoverClose(_)
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
            Self::State | Self::Shutdown => 1,
        }
    }
}
