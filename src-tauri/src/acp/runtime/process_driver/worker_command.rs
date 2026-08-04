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
        let bytes = command.actual_bytes().ok_or(MailboxError::Full)?;
        let control = command.is_control();
        let deadline_millis =
            monotonic_deadline_millis(Duration::from_millis(command.deadline_millis()));
        let mut retained = Some(command);
        let mut response = None;
        let build = || {
            let (reply, receiver) = mpsc::sync_channel(1);
            response = Some(receiver);
            ControlMessage::Worker(WorkerEnvelope {
                command: retained.take().expect("worker command retained"),
                reply,
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
            #[cfg(test)]
            timeout_override_millis: None,
        }
    }

    #[cfg(test)]
    fn new_with_timeout(
        owner: RuntimeOwner,
        mailbox: Arc<DriverMailbox>,
        deadline_abort: Arc<dyn WorkerDeadlineAbort>,
        timeout_millis: u64,
    ) -> Self {
        Self {
            owner,
            mailbox,
            deadline_abort,
            timeout_override_millis: Some(timeout_millis),
        }
    }

    fn call(&self, command: WorkerCommand) -> Result<WorkerReply, HostError> {
        let started = Instant::now();
        let deadline_millis = command.deadline_millis();
        let response = self.mailbox.submit_worker(command).map_err(map_mailbox)?;
        #[cfg(test)]
        let deadline_millis = self.timeout_override_millis.unwrap_or(deadline_millis);
        let remaining = Duration::from_millis(deadline_millis).saturating_sub(started.elapsed());
        match response.recv_timeout(remaining) {
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

    fn shutdown(&mut self) -> Result<(), HostError> {
        match self.call(WorkerCommand::Shutdown)? {
            WorkerReply::Unit => Ok(()),
            WorkerReply::Prepared(_) | WorkerReply::Write(_) | WorkerReply::Zero(_) => {
                Err(HostError::State)
            }
        }
    }
}

fn owner_bytes(owner: &RuntimeOwner) -> usize {
    owner.project_id.as_str().len()
        + owner.chat_id.as_str().len()
        + owner.session_id.as_str().len()
        + std::mem::size_of::<u64>()
}

fn close_bytes(command: &CloseSessionCommand) -> Option<usize> {
    owner_bytes(&RuntimeOwner {
        project_id: command.project_id.clone(),
        chat_id: command.chat_id.clone(),
        session_id: command.session_id.clone(),
        runtime_epoch: command.runtime_epoch,
    })
    .checked_add(command.client_intent_id.as_str().len())?
    .checked_add(std::mem::size_of::<u64>() + std::mem::size_of::<i64>())
}

fn map_mailbox(error: MailboxError) -> HostError {
    match error {
        MailboxError::Full | MailboxError::InvalidChunk => HostError::Capacity,
        MailboxError::Closed => HostError::Closed,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Instant,
    };

    use crate::acp::{
        contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
        limits::LIMITS,
    };

    use super::*;

    fn owner() -> RuntimeOwner {
        RuntimeOwner {
            project_id: SageProjectId::parse("project-deadline").unwrap(),
            chat_id: SageChatId::parse("chat-deadline").unwrap(),
            session_id: SageAgentSessionId::parse("session-deadline").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        }
    }

    #[test]
    fn nonresponding_worker_returns_at_deadline_and_closes_its_mailbox() {
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let exits = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&exits);
        let abort: Arc<dyn WorkerDeadlineAbort> = Arc::new(move |reason| {
            recorded.lock().unwrap().push(reason);
        });
        let port = ProcessLivePort::new_with_timeout(owner(), Arc::clone(&mailbox), abort, 20);
        let started = Instant::now();
        assert!(matches!(
            port.call(WorkerCommand::Shutdown),
            Err(HostError::Deadline)
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(matches!(
            mailbox.submit_worker(WorkerCommand::Shutdown),
            Err(MailboxError::Closed)
        ));
        assert_eq!(*exits.lock().unwrap(), [TreeExit::Deadline]);
    }

    #[test]
    fn worker_commands_use_the_frozen_operation_deadlines() {
        assert_eq!(
            WorkerCommand::WaitZero { owner: owner() }.deadline_millis(),
            LIMITS.deadlines_ms.explicit_close
        );
        assert_eq!(
            WorkerCommand::Shutdown.deadline_millis(),
            LIMITS.deadlines_ms.explicit_close
        );
    }
}
