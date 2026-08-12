use std::{
    sync::{Arc, OnceLock, mpsc},
    time::{Duration, Instant},
};

use crate::acp::{
    contract::SageTurnId,
    process::TreeExit,
    runtime::{HostCancelIntent, HostError, HostPromptIntent, LiveRuntimePort, LiveWrite},
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
};

use super::{ControlMessage, DriverMailbox, MailboxError, WorkerZeroProof};

mod support;
use support::{close_bytes, map_mailbox, owner_bytes};

pub(crate) enum WorkerCommand {
    PreparePrompt(HostPromptIntent),
    DispatchPrompt {
        owner: RuntimeOwner,
        turn_id: SageTurnId,
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
    BeginClose(CloseSessionCommand),
    WaitZero {
        owner: RuntimeOwner,
    },
    FinalizeClose {
        command: CloseSessionCommand,
        proof: WorkerZeroProof,
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
    pub(crate) command: WorkerCommand,
    reply: mpsc::SyncSender<Result<WorkerReply, HostError>>,
    deadline: Instant,
}

pub(crate) struct WorkerResponder(mpsc::SyncSender<Result<WorkerReply, HostError>>);

impl WorkerCommand {
    fn deadline_millis(&self) -> u64 {
        use crate::acp::limits::LIMITS;
        match self {
            Self::DispatchCancel(_) => LIMITS.deadlines_ms.cancel_dispatch,
            Self::BeginCancel(_)
            | Self::AcceptTerminal { .. }
            | Self::BeginClose(_)
            | Self::WaitZero { .. }
            | Self::FinalizeClose { .. }
            | Self::Shutdown => LIMITS.deadlines_ms.explicit_close,
            Self::PreparePrompt(_) | Self::DispatchPrompt { .. } | Self::ObserveSend { .. } => {
                LIMITS.deadlines_ms.ordinary_rpc
            }
        }
    }

    fn is_control(&self) -> bool {
        matches!(
            self,
            Self::ObserveSend { .. }
                | Self::BeginCancel(_)
                | Self::DispatchCancel(_)
                | Self::AcceptTerminal { .. }
                | Self::BeginClose(_)
                | Self::WaitZero { .. }
                | Self::FinalizeClose { .. }
                | Self::Shutdown
        )
    }

    fn actual_bytes(&self) -> Option<usize> {
        match self {
            Self::PreparePrompt(intent) => owner_bytes(&intent.owner())
                .checked_add(intent.command.client_intent_id.as_str().len())?
                .checked_add(intent.command.prompt.as_str().len())?
                .checked_add(std::mem::size_of::<u64>() * 2),
            Self::DispatchPrompt { owner, turn_id } | Self::ObserveSend { owner, turn_id, .. } => {
                owner_bytes(owner)
                    .checked_add(turn_id.as_str().len())?
                    .checked_add(std::mem::size_of::<SendObservation>())
            }
            Self::BeginCancel(intent) | Self::DispatchCancel(intent) => owner_bytes(&intent.owner)
                .checked_add(intent.turn_id.as_str().len())?
                .checked_add(intent.client_intent_id.as_str().len())?
                .checked_add(std::mem::size_of::<i64>()),
            Self::AcceptTerminal { owner, cursor } => owner_bytes(owner)
                .checked_add(cursor.turn_id().as_str().len())?
                .checked_add(32),
            Self::BeginClose(command) => close_bytes(command),
            Self::WaitZero { owner } => owner_bytes(owner).checked_add(1),
            Self::FinalizeClose { command, proof } => {
                close_bytes(command)?.checked_add(proof.actual_bytes()?)
            }
            Self::Shutdown => Some(1),
        }
    }
}

impl WorkerEnvelope {
    pub(crate) fn respond(self, reply: Result<WorkerReply, HostError>) {
        let _ = self.reply.send(reply);
    }

    pub(crate) fn reject(self, error: HostError) {
        self.respond(Err(error));
    }

    pub(crate) fn into_parts(self) -> (WorkerCommand, WorkerResponder) {
        (self.command, WorkerResponder(self.reply))
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
}

impl WorkerResponder {
    pub(crate) fn respond(self, reply: Result<WorkerReply, HostError>) {
        let _ = self.0.send(reply);
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

    fn submit_worker_until(
        &self,
        command: WorkerCommand,
        deadline: Instant,
    ) -> Result<mpsc::Receiver<Result<WorkerReply, HostError>>, MailboxError> {
        let bytes = command.actual_bytes().ok_or(MailboxError::Full)?;
        let control = command.is_control();
        let deadline_millis =
            monotonic_deadline_millis(deadline.saturating_duration_since(Instant::now()));
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
            self.try_push_data(bytes, build)?;
        }
        response.ok_or(MailboxError::Closed)
    }
}

fn monotonic_deadline_millis(after: Duration) -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_millis();
    elapsed
        .saturating_add(after.as_millis())
        .min(u128::from(u64::MAX)) as u64
}

pub(crate) struct ProcessLivePort {
    owner: RuntimeOwner,
    mailbox: Arc<DriverMailbox>,
    deadline_abort: Arc<dyn WorkerDeadlineAbort>,
    managed_runner: Option<Arc<super::LiveRunnerHandle>>,
    #[cfg(test)]
    timeout_override_millis: Option<u64>,
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
        }
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
        }
    }

    fn call(&self, command: WorkerCommand) -> Result<WorkerReply, HostError> {
        let deadline_millis = command.deadline_millis();
        #[cfg(test)]
        let deadline_millis = self.timeout_override_millis.unwrap_or(deadline_millis);
        let deadline = Instant::now() + Duration::from_millis(deadline_millis);
        self.call_until(command, deadline)
    }

    fn call_until(
        &self,
        command: WorkerCommand,
        deadline: Instant,
    ) -> Result<WorkerReply, HostError> {
        let response = self
            .mailbox
            .submit_worker_until(command, deadline)
            .map_err(map_mailbox)?;
        match response.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) => reply,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.mailbox.close();
                self.deadline_abort.request_termination(TreeExit::Deadline);
                Err(HostError::Deadline)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
    }
}

impl LiveRuntimePort for ProcessLivePort {
    type ZeroProof = WorkerZeroProof;

    fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        match self.call(WorkerCommand::PreparePrompt(intent.clone()))? {
            WorkerReply::Prepared(turn_id) => Ok(turn_id),
            WorkerReply::Write(_) | WorkerReply::Unit | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<LiveWrite, HostError> {
        match self.call(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_id.clone(),
        })? {
            WorkerReply::Write(write) => Ok(write),
            WorkerReply::Prepared(_) | WorkerReply::Unit | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn observe_send(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), HostError> {
        match self.call(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_id.clone(),
            observation,
        })? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        match self.call(WorkerCommand::BeginCancel(intent.clone()))? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn dispatch_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        match self.call(WorkerCommand::DispatchCancel(intent.clone()))? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn accept_terminal(&mut self, cursor: &AdapterTurnCursor) -> Result<(), HostError> {
        match self.call(WorkerCommand::AcceptTerminal {
            owner: self.owner.clone(),
            cursor: cursor.clone(),
        })? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn begin_close(&mut self, command: &CloseSessionCommand) -> Result<(), HostError> {
        match self.call(WorkerCommand::BeginClose(command.clone()))? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }

    fn wait_zero(&mut self, owner: &RuntimeOwner) -> Result<Self::ZeroProof, HostError> {
        match self.call(WorkerCommand::WaitZero {
            owner: owner.clone(),
        })? {
            WorkerReply::Zero(proof) => Ok(proof),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Unit => {
                Err(HostError::State)
            }
        }
    }

    fn finalize_close(
        &mut self,
        command: &CloseSessionCommand,
        proof: Self::ZeroProof,
    ) -> Result<(), (HostError, Self::ZeroProof)> {
        let retained = proof.clone();
        match self.call(WorkerCommand::FinalizeClose {
            command: command.clone(),
            proof,
        }) {
            Ok(WorkerReply::Unit) => Ok(()),
            Ok(WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_)) => {
                Err((HostError::State, retained))
            }
            Err(error) => Err((error, retained)),
        }
    }

    fn shutdown(&mut self, deadline: Instant) -> Result<(), HostError> {
        let reply = self.call_until(WorkerCommand::Shutdown, deadline);
        match reply {
            Ok(WorkerReply::Unit) => match &self.managed_runner {
                Some(runner) => runner.join_before(deadline),
                None => Ok(()),
            },
            Err(HostError::Closed) if self.managed_runner.is_some() => self
                .managed_runner
                .as_ref()
                .expect("managed runner checked")
                .shutdown_recovery_before(deadline)
                .map(|_| ()),
            Err(error) => Err(error),
            Ok(WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_)) => {
                Err(HostError::State)
            }
        }
    }
}

#[cfg(test)]
#[path = "worker_command_tests.rs"]
mod tests;
