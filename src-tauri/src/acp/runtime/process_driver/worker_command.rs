use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use crate::{
    acp::{
        contract::{SageServerRequestId, SageTurnId},
        process::TreeExit,
        runtime::{
            ConfigCompletionSink, HostCancelIntent, HostError, HostPromptIntent, HostSteerIntent,
            LiveWrite, NativeRpcCompletionSink, SteerCompletionSink,
        },
        scheduler::RuntimeOwner,
        store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
    },
    acp_protocol::permissions::PermissionDecision,
};

use super::{ControlMessage, DriverMailbox, MailboxError, WorkerZeroProof};

mod port;
mod support;
use support::{close_bytes, map_mailbox, owner_bytes};

pub(crate) enum WorkerCommand {
    ConfigureSession {
        intent: crate::acp::session_config::ConfigIntent,
        completion: ConfigCompletionSink,
    },
    NativeRpc {
        intent: crate::acp::goal::NativeRpcIntent,
        completion: NativeRpcCompletionSink,
    },
    AbandonConfiguration {
        owner: RuntimeOwner,
    },
    ResolvePermission {
        owner: RuntimeOwner,
        request_id: SageServerRequestId,
        decision: PermissionDecision,
    },
    PreparePrompt(HostPromptIntent),
    DispatchPrompt {
        owner: RuntimeOwner,
        turn_id: SageTurnId,
    },
    Steer {
        intent: HostSteerIntent,
        completion: SteerCompletionSink,
    },
    ObserveSend {
        owner: RuntimeOwner,
        turn_id: SageTurnId,
        observation: SendObservation,
    },
    BeginCancel(HostCancelIntent),
    DispatchCancel(HostCancelIntent),
    AcceptTerminal {
        owner: RuntimeOwner,
        cursor: AdapterTurnCursor,
    },
    AcceptLatestCompleted,
    BeginClose(CloseSessionCommand),
    WaitZero {
        owner: RuntimeOwner,
    },
    FinalizeClose {
        command: CloseSessionCommand,
        proof: WorkerZeroProof,
    },
    BeginPark,
    ParkAfterZero {
        owner: RuntimeOwner,
    },
    Shutdown,
}

pub(crate) enum WorkerReply {
    Prepared(SageTurnId),
    Write(LiveWrite),
    Unit,
    Zero(WorkerZeroProof),
}

pub(crate) struct WorkerEnvelope {
    pub(super) command: WorkerCommand,
    pub(super) reply: mpsc::SyncSender<Result<WorkerReply, HostError>>,
    pub(super) deadline: Instant,
}

pub(crate) struct WorkerResponder(pub(super) mpsc::SyncSender<Result<WorkerReply, HostError>>);

impl WorkerCommand {
    fn deadline_millis(&self) -> u64 {
        use sage_core::limits::LIMITS;
        match self {
            Self::ConfigureSession { .. }
            | Self::NativeRpc { .. }
            | Self::AbandonConfiguration { .. }
            | Self::ResolvePermission { .. } => LIMITS.deadlines_ms.ordinary_rpc,
            Self::DispatchCancel(_) => LIMITS.deadlines_ms.cancel_dispatch,
            Self::BeginCancel(_)
            | Self::AcceptTerminal { .. }
            | Self::AcceptLatestCompleted
            | Self::BeginClose(_)
            | Self::WaitZero { .. }
            | Self::FinalizeClose { .. }
            | Self::BeginPark
            | Self::ParkAfterZero { .. }
            | Self::Shutdown => LIMITS.deadlines_ms.explicit_close,
            Self::PreparePrompt(_)
            | Self::DispatchPrompt { .. }
            | Self::Steer { .. }
            | Self::ObserveSend { .. } => LIMITS.deadlines_ms.ordinary_rpc,
        }
    }

    /// A command that holds a user prompt, with whatever images it carries.
    fn carries_prompt(&self) -> bool {
        matches!(self, Self::PreparePrompt(_) | Self::Steer { .. })
    }

    fn is_control(&self) -> bool {
        matches!(
            self,
            Self::AbandonConfiguration { .. }
                | Self::ResolvePermission { .. }
                | Self::ObserveSend { .. }
                | Self::BeginCancel(_)
                | Self::DispatchCancel(_)
                | Self::AcceptTerminal { .. }
                | Self::AcceptLatestCompleted
                | Self::BeginClose(_)
                | Self::WaitZero { .. }
                | Self::FinalizeClose { .. }
                | Self::BeginPark
                | Self::ParkAfterZero { .. }
                | Self::Shutdown
        )
    }

    fn actual_bytes(&self) -> Option<usize> {
        match self {
            Self::ConfigureSession { intent, .. } => Some(intent.actual_bytes()),
            Self::NativeRpc { intent, .. } => Some(intent.actual_bytes()),
            Self::AbandonConfiguration { owner } => owner_bytes(owner).checked_add(1),
            Self::ResolvePermission {
                owner,
                request_id,
                decision,
            } => owner_bytes(owner)
                .checked_add(request_id.as_str().len())?
                .checked_add(match decision {
                    PermissionDecision::Answer(answers) => answers.retained_bytes(),
                    _ => 0,
                })?
                .checked_add(1),
            Self::PreparePrompt(intent) => owner_bytes(&intent.owner())
                .checked_add(intent.command.client_intent_id.as_str().len())?
                .checked_add(intent.command.prompt.retained_bytes())?
                .checked_add(std::mem::size_of::<u64>() * 2),
            Self::DispatchPrompt { owner, turn_id } | Self::ObserveSend { owner, turn_id, .. } => {
                owner_bytes(owner)
                    .checked_add(turn_id.as_str().len())?
                    .checked_add(std::mem::size_of::<SendObservation>())
            }
            Self::Steer { intent, .. } => owner_bytes(&intent.owner)
                .checked_add(intent.turn_id.as_str().len())?
                .checked_add(intent.prompt.retained_bytes())?
                .checked_add(intent.client_message_id.as_str().len()),
            Self::BeginCancel(intent) | Self::DispatchCancel(intent) => owner_bytes(&intent.owner)
                .checked_add(intent.turn_id.as_str().len())?
                .checked_add(intent.client_intent_id.as_str().len())?
                .checked_add(std::mem::size_of::<i64>()),
            Self::AcceptTerminal { owner, cursor } => owner_bytes(owner)
                .checked_add(cursor.turn_id().as_str().len())?
                .checked_add(32),
            Self::BeginClose(command) => close_bytes(command),
            Self::WaitZero { owner } | Self::ParkAfterZero { owner } => {
                owner_bytes(owner).checked_add(1)
            }
            Self::FinalizeClose { command, proof } => {
                close_bytes(command)?.checked_add(proof.actual_bytes()?)
            }
            Self::AcceptLatestCompleted | Self::BeginPark | Self::Shutdown => Some(1),
        }
    }
}

impl DriverMailbox {
    pub(crate) fn submit_worker(
        &self,
        command: WorkerCommand,
    ) -> Result<mpsc::Receiver<Result<WorkerReply, HostError>>, MailboxError> {
        let deadline = Instant::now() + Duration::from_millis(command.deadline_millis());
        self.submit_worker_until(command, deadline)
    }

    pub(super) fn submit_worker_until(
        &self,
        command: WorkerCommand,
        deadline: Instant,
    ) -> Result<mpsc::Receiver<Result<WorkerReply, HostError>>, MailboxError> {
        let bytes = command.actual_bytes().ok_or(MailboxError::Full)?;
        let control = command.is_control();
        let prompt = command.carries_prompt();
        let deadline_millis = super::driver::monotonic_deadline_millis(
            deadline.saturating_duration_since(Instant::now()),
        );
        let mut retained = Some(command);
        let mut response = None;
        let build = || {
            let (reply, receiver) = mpsc::sync_channel(1);
            response = Some(receiver);
            ControlMessage::Worker(WorkerEnvelope {
                command: retained.take().expect("worker command retained"),
                reply,
                deadline,
            })
        };
        if control {
            self.try_push_control(bytes, deadline_millis, build)?;
        } else {
            self.try_push_data(bytes, prompt, build)?;
        }
        response.ok_or(MailboxError::Closed)
    }
}

pub(crate) struct ProcessLivePort {
    pub(super) owner: RuntimeOwner,
    mailbox: Arc<DriverMailbox>,
    deadline_abort: Arc<dyn WorkerDeadlineAbort>,
    pub(super) managed_runner: Option<Arc<super::LiveRunnerHandle>>,
    #[cfg(test)]
    timeout_override_millis: Option<u64>,
    #[cfg(test)]
    finish_park_failure: bool,
}

pub(crate) trait WorkerDeadlineAbort: Send + Sync + 'static {
    fn request_termination(&self, reason: TreeExit);
}

impl<F> WorkerDeadlineAbort for F
where
    F: Fn(TreeExit) + Send + Sync + 'static,
{
    fn request_termination(&self, reason: TreeExit) {
        self(reason);
    }
}

impl ProcessLivePort {
    #[cfg(test)]
    pub(crate) fn new(
        owner: RuntimeOwner,
        mailbox: Arc<DriverMailbox>,
        deadline_abort: Arc<dyn WorkerDeadlineAbort>,
    ) -> Self {
        Self {
            owner,
            mailbox,
            deadline_abort,
            managed_runner: None,
            #[cfg(test)]
            timeout_override_millis: None,
            #[cfg(test)]
            finish_park_failure: false,
        }
    }

    pub(crate) fn new_managed(owner: RuntimeOwner, runner: Arc<super::LiveRunnerHandle>) -> Self {
        let mailbox = runner.mailbox();
        let deadline_abort: Arc<dyn WorkerDeadlineAbort> = runner.clone();
        Self {
            owner,
            mailbox,
            deadline_abort,
            managed_runner: Some(runner),
            #[cfg(test)]
            timeout_override_millis: None,
            #[cfg(test)]
            finish_park_failure: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_finish_park_failure(&mut self) {
        self.finish_park_failure = true;
    }

    #[cfg(test)]
    pub(super) fn new_with_timeout(
        owner: RuntimeOwner,
        mailbox: Arc<DriverMailbox>,
        deadline_abort: Arc<dyn WorkerDeadlineAbort>,
        timeout_millis: u64,
    ) -> Self {
        Self {
            owner,
            mailbox,
            deadline_abort,
            managed_runner: None,
            timeout_override_millis: Some(timeout_millis),
            finish_park_failure: false,
        }
    }

    pub(super) fn call(&self, command: WorkerCommand) -> Result<WorkerReply, HostError> {
        let deadline_millis = command.deadline_millis();
        #[cfg(test)]
        let deadline_millis = self.timeout_override_millis.unwrap_or(deadline_millis);
        let deadline = Instant::now() + Duration::from_millis(deadline_millis);
        self.call_until(command, deadline)
    }

    pub(super) fn call_until(
        &self,
        command: WorkerCommand,
        deadline: Instant,
    ) -> Result<WorkerReply, HostError> {
        let started = Instant::now();
        let kind = match &command {
            WorkerCommand::PreparePrompt(_) => "prepare_prompt",
            WorkerCommand::DispatchPrompt { .. } => "dispatch_prompt",
            WorkerCommand::ObserveSend { .. } => "observe_send",
            _ => "control",
        };
        let budget_ms = deadline.saturating_duration_since(started).as_millis();
        let response = self
            .mailbox
            .submit_worker_until(command, deadline)
            .map_err(map_mailbox)
            .inspect_err(|error| {
                crate::acp::runtime::prompt_diagnostics::failed(
                    "port.submit",
                    started,
                    error,
                    (kind, budget_ms, self.mailbox.diagnostic_state()),
                );
            })?;
        match response.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) => reply,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.mailbox.close();
                self.deadline_abort.request_termination(TreeExit::Deadline);
                Err(HostError::Deadline)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
        .inspect_err(|error| {
            crate::acp::runtime::prompt_diagnostics::failed(
                "port.reply",
                started,
                error,
                (kind, budget_ms, self.mailbox.diagnostic_state()),
            );
        })
    }
}

#[cfg(test)]
#[path = "worker_command_tests.rs"]
mod tests;
