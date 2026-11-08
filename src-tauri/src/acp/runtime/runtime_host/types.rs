use crate::acp::{
    contract::{ClientIntentId, RequiredText, SageProjectId, SageServerRequestId, SageTurnId},
    intent::Prompt,
    runtime::permissions::PermissionDecision,
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, PrepareTurnCommand},
};
use std::{sync::mpsc, time::Instant};

mod command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostState {
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
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
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
    PromptPrepared,
    PromptAccepted,
    SteerQueued,
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
    #[error("the harness rejected the native Goal mutation")]
    GoalMutationRejected,
    #[error("the harness requires sign-in before it can continue")]
    SignInRequired,
    #[error("prompt delivery may have written bytes and cannot be replayed")]
    DeliveryUnknown,
    #[error("the one permitted zero-byte prompt retry is exhausted")]
    RetryExhausted,
    #[error("runtime ownership requires authoritative recovery")]
    RecoveryRequired,
    #[error("the runtime operation exceeded its bounded deadline")]
    Deadline,
    /// A config change or native RPC whose deadline passed before it was
    /// written: nothing reached the harness and the host is unharmed.
    #[error("the runtime operation's deadline passed before it was sent")]
    NotDispatched,
    #[error("the Steer acknowledgement timed out; the message may have been delivered")]
    SteerOutcomeUnknown,
    #[error("the runtime host is closed")]
    Closed,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct HostPromptIntent {
    pub(crate) command: PrepareTurnCommand,
    pub(crate) access_mode: crate::settings_store::SessionAccessMode,
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
pub(crate) struct HostSteerIntent {
    pub(crate) owner: RuntimeOwner,
    pub(crate) turn_id: SageTurnId,
    pub(crate) prompt: Prompt,
    pub(crate) client_message_id: RequiredText<512>,
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

type OperationCompletionCallback<T> = Box<dyn FnOnce(Result<T, HostError>) + Send + 'static>;

pub(crate) struct OperationCompletionSink<T>(Option<OperationCompletionCallback<T>>);
pub(crate) type ConfigCompletionSink = OperationCompletionSink<()>;
pub(crate) type SteerCompletionSink = OperationCompletionSink<()>;
pub(crate) type NativeRpcCompletionSink = OperationCompletionSink<crate::acp::goal::NativeRpcReply>;

impl<T> OperationCompletionSink<T> {
    pub(crate) fn new(complete: impl FnOnce(Result<T, HostError>) + Send + 'static) -> Self {
        Self(Some(Box::new(complete)))
    }

    pub(crate) fn complete(mut self, result: Result<T, HostError>) {
        if let Some(complete) = self.0.take() {
            complete(result);
        }
    }
}

impl<T> Drop for OperationCompletionSink<T> {
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

pub(super) struct NativeRpcCompletion {
    pub(super) operation_id: u64,
    pub(super) owner: RuntimeOwner,
    pub(super) result: Result<crate::acp::goal::NativeRpcReply, HostError>,
}

pub(super) struct SteerCompletion {
    pub(super) operation_id: u64,
    pub(super) owner: RuntimeOwner,
    pub(super) result: Result<(), HostError>,
}

pub(super) struct PendingHostSteer {
    pub(super) operation_id: u64,
    pub(super) owner: RuntimeOwner,
    pub(super) reply: mpsc::SyncSender<Result<HostReply, HostError>>,
    pub(super) deadline: Instant,
}

pub(super) struct PendingHostConfig {
    pub(super) operation_id: u64,
    pub(super) owner: RuntimeOwner,
    pub(super) reply: mpsc::SyncSender<Result<HostReply, HostError>>,
    pub(super) deadline: Instant,
    pub(super) native_rpc: bool,
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
    fn begin_native_rpc(
        &mut self,
        _intent: &crate::acp::goal::NativeRpcIntent,
        _deadline: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(Err(HostError::State));
        Ok(())
    }
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
    fn dispatch_steer(&mut self, _intent: &HostSteerIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn begin_steer(
        &mut self,
        intent: &HostSteerIntent,
        _deadline: Instant,
        completion: SteerCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(self.dispatch_steer(intent));
        Ok(())
    }
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
    NativeRpc(crate::acp::goal::NativeRpcIntent),
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
    Steer(HostSteerIntent),
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
    NativeRpc(crate::acp::goal::NativeRpcReply),
    Prepared(SageTurnId),
    #[allow(dead_code, reason = "only tests query the host state")]
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
            Ok(Ok(
                HostReply::Outcome(_)
                | HostReply::NativeRpc(_)
                | HostReply::Prepared(_)
                | HostReply::State(_),
            )) => Err(HostError::State),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(HostError::Deadline),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
    }
}
