use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use crate::acp::{scheduler::RuntimeOwner, store::AdapterTurnCursor};

use super::super::bounded_thread::{BoundedThread, ThreadWaitError};
use super::{
    actor::Actor,
    mailbox::{Envelope, HostMailbox},
    types::{
        HostCancelIntent, HostCloseIntent, HostCommand, HostError, HostOutcome, HostPromptIntent,
        HostReply, HostState, RuntimeEngine,
    },
};

pub(crate) struct RuntimeHost {
    pub(super) mailbox: Arc<HostMailbox>,
    thread: Mutex<Option<BoundedThread>>,
    lifecycle: AtomicU8,
    #[cfg(test)]
    timeout_override: Option<TimeoutOverride>,
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum TimeoutOverride {
    Duration(Duration),
    Deadline {
        operation: Instant,
        cleanup: Instant,
    },
}

impl RuntimeHost {
    pub(crate) fn start<E: RuntimeEngine>(engine: E) -> Result<Self, HostError> {
        Self::start_inner(engine, None)
    }

    #[cfg(test)]
    pub(crate) fn start_with_timeout<E: RuntimeEngine>(
        engine: E,
        timeout: Duration,
    ) -> Result<Self, HostError> {
        Self::start_inner(engine, Some(TimeoutOverride::Duration(timeout)))
    }

    #[cfg(all(test, windows))]
    pub(crate) fn start_with_deadline<E: RuntimeEngine>(
        engine: E,
        operation: Instant,
        cleanup: Instant,
    ) -> Result<Self, HostError> {
        Self::start_inner(
            engine,
            Some(TimeoutOverride::Deadline { operation, cleanup }),
        )
    }

    fn start_inner<E: RuntimeEngine>(
        engine: E,
        #[cfg(test)] timeout_override: Option<TimeoutOverride>,
        #[cfg(not(test))] _timeout_override: Option<Duration>,
    ) -> Result<Self, HostError> {
        let mailbox = Arc::new(HostMailbox::new()?);
        let actor_mailbox = Arc::clone(&mailbox);
        let thread = BoundedThread::spawn("sage-acp-runtime-host", move || {
            Actor::new(engine).run(&actor_mailbox)
        })
        .map_err(|_| HostError::Unavailable)?;
        Ok(Self {
            mailbox,
            thread: Mutex::new(Some(thread)),
            lifecycle: AtomicU8::new(HOST_RUNNING),
            #[cfg(test)]
            timeout_override,
        })
    }

    pub(crate) fn activate(&self, owner: RuntimeOwner) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Activate(owner))
    }

    pub(crate) fn prepare_prompt(
        &self,
        intent: HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, HostError> {
        match self.submit(HostCommand::PreparePrompt(intent))? {
            HostReply::Prepared(turn_id) => Ok(turn_id),
            HostReply::Outcome(_) | HostReply::State(_) => Err(HostError::State),
        }
    }

    pub(crate) fn dispatch_prepared(
        &self,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::DispatchPrompt { owner, turn_id })
    }

    pub(crate) fn send(&self, intent: HostPromptIntent) -> Result<HostOutcome, HostError> {
        let owner = intent.owner();
        let turn_id = self.prepare_prompt(intent)?;
        self.dispatch_prepared(owner, turn_id)
    }

    pub(crate) fn cancel(&self, intent: HostCancelIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Cancel(intent))
    }

    pub(crate) fn accept_terminal(
        &self,
        owner: RuntimeOwner,
        cursor: AdapterTurnCursor,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::Terminal { owner, cursor })
    }

    pub(crate) fn project_switched(
        &self,
        project: crate::acp::contract::SageProjectId,
    ) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::ProjectSwitched(project))
    }

    pub(crate) fn state(&self) -> Result<HostState, HostError> {
        match self.submit(HostCommand::State)? {
            HostReply::State(state) => Ok(state),
            HostReply::Outcome(_) | HostReply::Prepared(_) => Err(HostError::State),
        }
    }

    pub(crate) fn begin_close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::BeginClose(intent))
    }

    pub(crate) fn finish_close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::FinishClose(intent))
    }

    pub(crate) fn recover_close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        self.outcome(HostCommand::RecoverClose(intent))
    }

    pub(crate) fn close(&self, intent: HostCloseIntent) -> Result<HostOutcome, HostError> {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        self.outcome_with_timeout(HostCommand::BeginClose(intent.clone()), budget)?;
        self.outcome_with_timeout(
            HostCommand::FinishClose(intent),
            budget.saturating_sub(started.elapsed()),
        )
    }

    pub(crate) fn shutdown(&self) -> Result<(), HostError> {
        let started = Instant::now();
        let budget = self.cleanup_timeout();
        match self.lifecycle.load(Ordering::Acquire) {
            HOST_JOINED => return Ok(()),
            HOST_AWAITING_EXIT | HOST_SHUTDOWN_ACKNOWLEDGED => {
                return self.join_with_timeout(budget);
            }
            HOST_RUNNING => {}
            _ => return Err(HostError::RecoveryRequired),
        }
        match self.outcome(HostCommand::Shutdown)? {
            HostOutcome::Shutdown => {
                self.lifecycle
                    .store(HOST_SHUTDOWN_ACKNOWLEDGED, Ordering::Release);
                self.join_with_timeout(budget.saturating_sub(started.elapsed()))
            }
            _ => Err(HostError::State),
        }
    }

    fn outcome(&self, command: HostCommand) -> Result<HostOutcome, HostError> {
        match self.submit(command)? {
            HostReply::Outcome(outcome) => Ok(outcome),
            HostReply::Prepared(_) | HostReply::State(_) => Err(HostError::State),
        }
    }

    fn outcome_with_timeout(
        &self,
        command: HostCommand,
        timeout: Duration,
    ) -> Result<HostOutcome, HostError> {
        match self.submit_with_timeout(command, timeout)? {
            HostReply::Outcome(outcome) => Ok(outcome),
            HostReply::Prepared(_) | HostReply::State(_) => Err(HostError::State),
        }
    }

    fn submit(&self, command: HostCommand) -> Result<HostReply, HostError> {
        let timeout = self.response_timeout(&command);
        self.submit_with_timeout(command, timeout)
    }

    fn submit_with_timeout(
        &self,
        command: HostCommand,
        timeout: Duration,
    ) -> Result<HostReply, HostError> {
        let started = Instant::now();
        match self.lifecycle.load(Ordering::Acquire) {
            HOST_RUNNING => {}
            HOST_AWAITING_EXIT => return Err(HostError::RecoveryRequired),
            HOST_SHUTDOWN_ACKNOWLEDGED | HOST_JOINED => return Err(HostError::Closed),
            _ => return Err(HostError::RecoveryRequired),
        }
        if timeout.is_zero() {
            self.fail_deadline();
            return Err(HostError::Deadline);
        }
        let (reply, response) = mpsc::sync_channel(1);
        self.mailbox.try_push(Envelope { command, reply })?;
        match response.recv_timeout(timeout.saturating_sub(started.elapsed())) {
            Ok(reply) => reply,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.fail_deadline();
                Err(HostError::Deadline)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.lifecycle.store(HOST_AWAITING_EXIT, Ordering::Release);
                self.mailbox.close();
                Err(HostError::Closed)
            }
        }
    }

    fn fail_deadline(&self) {
        let _ = self.lifecycle.compare_exchange(
            HOST_RUNNING,
            HOST_AWAITING_EXIT,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.mailbox.close();
    }

    fn response_timeout(&self, command: &HostCommand) -> Duration {
        #[cfg(test)]
        if let Some(timeout) = self.timeout_override {
            return match timeout {
                TimeoutOverride::Duration(duration) => duration,
                TimeoutOverride::Deadline { operation, .. } => operation
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(command.deadline_millis())),
            };
        }
        Duration::from_millis(command.deadline_millis())
    }

    pub(super) fn join(&self) -> Result<(), HostError> {
        self.join_with_timeout(self.cleanup_timeout())
    }

    fn cleanup_timeout(&self) -> Duration {
        let native = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        #[cfg(test)]
        if let Some(TimeoutOverride::Deadline { cleanup, .. }) = self.timeout_override {
            return native.min(cleanup.saturating_duration_since(Instant::now()));
        }
        native
    }

    fn join_with_timeout(&self, timeout: Duration) -> Result<(), HostError> {
        let mut thread = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(worker) = thread.as_mut() else {
            return Ok(());
        };
        match worker.wait(timeout) {
            Ok(()) => {
                *thread = None;
                self.lifecycle.store(HOST_JOINED, Ordering::Release);
                Ok(())
            }
            Err(ThreadWaitError::Timeout) => Err(HostError::Deadline),
            Err(ThreadWaitError::Panicked) => {
                *thread = None;
                self.lifecycle.store(HOST_JOINED, Ordering::Release);
                Err(HostError::Unavailable)
            }
        }
    }
}

impl Drop for RuntimeHost {
    fn drop(&mut self) {
        if self.thread.get_mut().is_ok_and(|thread| thread.is_none()) {
            return;
        }
        let started = Instant::now();
        let budget = self.cleanup_timeout();
        if self.lifecycle.load(Ordering::Acquire) == HOST_RUNNING
            && self.outcome(HostCommand::Shutdown).is_ok()
        {
            self.lifecycle
                .store(HOST_SHUTDOWN_ACKNOWLEDGED, Ordering::Release);
        } else {
            self.mailbox.close();
        }
        let _ = self.join_with_timeout(budget.saturating_sub(started.elapsed()));
    }
}

const HOST_RUNNING: u8 = 0;
const HOST_AWAITING_EXIT: u8 = 1;
const HOST_SHUTDOWN_ACKNOWLEDGED: u8 = 2;
const HOST_JOINED: u8 = 3;
