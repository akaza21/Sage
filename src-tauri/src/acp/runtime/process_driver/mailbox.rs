use std::{
    sync::mpsc,
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

use crate::acp::{
    contract::{
        RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
    },
    limits::LIMITS,
    scheduler::RuntimeOwner,
    transport::{QueueError, ReservedQueue},
};

use super::{DriverClock, DriverError, DriverProgress};

mod queue;
use queue::{ReceiveMode, map_queue, pop_driver_message, reader_chunk_limit};

pub(crate) struct PromptIntent {
    pub(crate) prompt: crate::acp::intent::Prompt,
    pub(crate) client_message_id: RequiredText<512>,
    pub(crate) reply: mpsc::SyncSender<Result<DriverProgress, DriverError>>,
}

impl PromptIntent {
    #[cfg(test)]
    fn actual_bytes(&self) -> Option<usize> {
        self.prompt
            .as_str()
            .len()
            .checked_add(self.client_message_id.as_str().len())
    }

    pub(super) fn reject(self, error: DriverError) {
        let _ = self.reply.send(Err(error));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CancelIntent {
    pub(crate) project_id: SageProjectId,
    pub(crate) chat_id: SageChatId,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) turn_id: SageTurnId,
    pub(crate) client_message_id: RequiredText<512>,
}

impl CancelIntent {
    pub(super) fn matches_owner(&self, owner: &RuntimeOwner) -> bool {
        self.project_id == owner.project_id
            && self.chat_id == owner.chat_id
            && self.session_id == owner.session_id
            && self.runtime_epoch == owner.runtime_epoch
    }

    #[cfg(test)]
    fn actual_bytes(&self) -> Option<usize> {
        self.project_id
            .as_str()
            .len()
            .checked_add(self.chat_id.as_str().len())?
            .checked_add(self.session_id.as_str().len())?
            .checked_add(self.turn_id.as_str().len())?
            .checked_add(self.client_message_id.as_str().len())?
            .checked_add(std::mem::size_of::<u64>())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReaderTerminal {
    Eof,
    Io,
}

pub(crate) enum ControlMessage {
    StdoutChunk(Vec<u8>),
    #[allow(dead_code, reason = "only tests construct this variant so far")]
    Prompt(PromptIntent),
    #[allow(dead_code, reason = "only tests construct this variant so far")]
    Cancel(CancelIntent),
    Worker(super::WorkerEnvelope),
    ReaderTerminal(ReaderTerminal),
    ProcessExit(crate::acp::process::TreeExit),
}

impl ControlMessage {
    fn reject_closed(self) {
        match self {
            Self::Prompt(intent) => {
                intent.reject(DriverError::Mailbox(MailboxError::Closed));
            }
            Self::Worker(envelope) => {
                envelope.reject(crate::acp::runtime::HostError::Closed);
            }
            Self::StdoutChunk(_)
            | Self::Cancel(_)
            | Self::ReaderTerminal(_)
            | Self::ProcessExit(_) => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MailboxError {
    Full,
    Closed,
    InvalidChunk,
}

struct State {
    queue: ReservedQueue<ControlMessage>,
    closed: bool,
    terminal_queued: bool,
}

pub(crate) struct DriverMailbox {
    state: Mutex<State>,
    ready: Condvar,
    space: Condvar,
}

impl DriverMailbox {
    pub(crate) fn new() -> Result<Self, MailboxError> {
        let queue = ReservedQueue::new(
            (LIMITS.queues.driver_data_messages + LIMITS.queues.driver_control_messages) as usize,
            (LIMITS.queues.driver_data_bytes + LIMITS.queues.driver_control_bytes) as usize,
            LIMITS.queues.driver_control_messages as usize,
            LIMITS.queues.driver_control_bytes as usize,
        )
        .map_err(map_queue)?;
        Ok(Self {
            state: Mutex::new(State {
                queue,
                closed: false,
                terminal_queued: false,
            }),
            ready: Condvar::new(),
            space: Condvar::new(),
        })
    }

    pub(crate) fn push_chunk_blocking(&self, chunk: Vec<u8>) -> Result<(), MailboxError> {
        if chunk.is_empty() || chunk.len() > reader_chunk_limit() {
            return Err(MailboxError::InvalidChunk);
        }
        let bytes = chunk.len();
        let mut retained = Some(chunk);
        let mut state = self.lock();
        loop {
            if state.closed {
                return Err(MailboxError::Closed);
            }
            let result = state.queue.try_push_data_with(bytes, || {
                ControlMessage::StdoutChunk(retained.take().unwrap_or_default())
            });
            match result {
                Ok(()) => {
                    self.ready.notify_one();
                    return Ok(());
                }
                Err(QueueError::CountCapacity | QueueError::ByteCapacity) => {
                    state = self
                        .space
                        .wait(state)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
                Err(_) => return Err(MailboxError::Full),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn try_push_chunk(&self, chunk: Vec<u8>) -> Result<(), MailboxError> {
        if chunk.is_empty() || chunk.len() > reader_chunk_limit() {
            return Err(MailboxError::InvalidChunk);
        }
        let bytes = chunk.len();
        let mut state = self.lock();
        if state.closed {
            return Err(MailboxError::Closed);
        }
        state
            .queue
            .try_push_data_with(bytes, || ControlMessage::StdoutChunk(chunk))
            .map_err(map_queue)?;
        self.ready.notify_one();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn submit_cancel(
        &self,
        intent: CancelIntent,
        deadline_ms: u64,
    ) -> Result<(), MailboxError> {
        let bytes = intent.actual_bytes().ok_or(MailboxError::Full)?;
        self.try_push_control(bytes, deadline_ms, || ControlMessage::Cancel(intent))
    }

    #[cfg(test)]
    pub(crate) fn submit_prompt(
        &self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: RequiredText<512>,
    ) -> Result<mpsc::Receiver<Result<DriverProgress, DriverError>>, MailboxError> {
        let (reply, response) = mpsc::sync_channel(1);
        let intent = PromptIntent {
            prompt,
            client_message_id,
            reply,
        };
        let bytes = intent.actual_bytes().ok_or(MailboxError::Full)?;
        let mut state = self.lock();
        if state.closed || state.terminal_queued {
            return Err(MailboxError::Closed);
        }
        state
            .queue
            .try_push_data_with(bytes, || ControlMessage::Prompt(intent))
            .map_err(map_queue)?;
        self.ready.notify_one();
        Ok(response)
    }

    pub(crate) fn signal_process_exit(
        &self,
        exit: crate::acp::process::TreeExit,
    ) -> Result<(), MailboxError> {
        self.try_push_control(1, 0, || ControlMessage::ProcessExit(exit))
    }

    pub(crate) fn signal_reader_terminal(
        &self,
        terminal: ReaderTerminal,
    ) -> Result<(), MailboxError> {
        self.push_ordered_reader_terminal(terminal)
    }

    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        while let Some(message) = state.queue.pop() {
            message.reject_closed();
        }
        state.terminal_queued = false;
        self.ready.notify_all();
        self.space.notify_all();
    }

    pub(super) fn wait_next_until(
        &self,
        deadline_millis: Option<i64>,
        clock: &dyn DriverClock,
    ) -> Result<Option<ControlMessage>, MailboxError> {
        self.wait_until(deadline_millis, clock, ReceiveMode::All)
    }

    pub(super) fn wait_control_until(
        &self,
        deadline_millis: Option<i64>,
        clock: &dyn DriverClock,
    ) -> Result<Option<ControlMessage>, MailboxError> {
        self.wait_until(deadline_millis, clock, ReceiveMode::Control)
    }

    pub(super) fn wait_write_observation_until(
        &self,
        deadline_millis: Option<i64>,
        clock: &dyn DriverClock,
    ) -> Result<Option<ControlMessage>, MailboxError> {
        self.wait_until(deadline_millis, clock, ReceiveMode::WriteObservation)
    }

    fn wait_until(
        &self,
        deadline_millis: Option<i64>,
        clock: &dyn DriverClock,
        mode: ReceiveMode,
    ) -> Result<Option<ControlMessage>, MailboxError> {
        let mut state = self.lock();
        loop {
            let message = pop_driver_message(&mut state.queue, mode);
            if let Some(message) = message {
                self.space.notify_all();
                return Ok(Some(message));
            }
            if state.closed {
                return Err(MailboxError::Closed);
            }
            let Some(deadline) = deadline_millis else {
                state = self
                    .ready
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                continue;
            };
            let now = clock.now_millis();
            if now >= deadline {
                return Ok(None);
            }
            let remaining = u64::try_from(deadline.saturating_sub(now)).unwrap_or(u64::MAX);
            let started = Instant::now();
            let (next, timed) = self
                .ready
                .wait_timeout(state, Duration::from_millis(remaining))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            if timed.timed_out() && started.elapsed() >= Duration::from_millis(remaining) {
                return Ok(None);
            }
        }
    }

    pub(super) fn try_pop_control(&self) -> Option<ControlMessage> {
        let mut state = self.lock();
        let stdout_pending = matches!(
            state.queue.data_front(),
            Some(ControlMessage::StdoutChunk(_))
        );
        let message = state.queue.pop_control_if(|message| {
            !(stdout_pending && matches!(message, ControlMessage::ProcessExit(_)))
        });
        if message.is_some() {
            self.space.notify_all();
        }
        message
    }

    /// Allows a completed Turn to hand its session to the reusable worker.
    /// Native stdout queued here follows the durable terminal in stream
    /// order, and the reusable session's idle ingress reads it next, exactly
    /// as if the terminal callback had been dequeued after it. Queued control,
    /// a reader terminal or a closed mailbox still block the handoff.
    pub(super) fn admits_completed_turn_handoff(&self) -> bool {
        let state = self.lock();
        !state.terminal_queued
            && !state.closed
            && state
                .queue
                .all_items_match(|message| matches!(message, ControlMessage::StdoutChunk(_)))
    }

    pub(super) fn reader_terminal_queued(&self) -> bool {
        self.lock().terminal_queued
    }

    /// Allows only already-authorized worker operations to cross the atomic
    /// activation-to-idle ownership boundary. Raw stdout must be drained by
    /// activation first, so an unknown frame or reader terminal still prevents
    /// the handoff and is handled fail-closed by the activation driver.
    pub(super) fn is_activation_handoff_ready(&self) -> bool {
        let state = self.lock();
        !state.terminal_queued
            && !state.closed
            && state
                .queue
                .all_items_match(|message| matches!(message, ControlMessage::Worker(_)))
    }

    #[cfg(test)]
    pub(super) fn retained_usage(&self) -> ((usize, usize), (usize, usize)) {
        let state = self.lock();
        (state.queue.data_usage(), state.queue.control_usage())
    }

    fn push_ordered_reader_terminal(&self, terminal: ReaderTerminal) -> Result<(), MailboxError> {
        let mut retained = Some(terminal);
        let mut state = self.lock();
        if state.terminal_queued {
            return Ok(());
        }
        loop {
            if state.closed {
                return Err(MailboxError::Closed);
            }
            let result = state.queue.try_push_data_with(1, || {
                ControlMessage::ReaderTerminal(retained.take().unwrap_or(ReaderTerminal::Io))
            });
            match result {
                Ok(()) => {
                    state.terminal_queued = true;
                    self.ready.notify_one();
                    return Ok(());
                }
                Err(QueueError::CountCapacity | QueueError::ByteCapacity) => {
                    state = self
                        .space
                        .wait(state)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
                Err(_) => return Err(MailboxError::Full),
            }
        }
    }

    pub(super) fn try_push_data(
        &self,
        bytes: usize,
        build: impl FnOnce() -> ControlMessage,
    ) -> Result<(), MailboxError> {
        let mut state = self.lock();
        if state.closed || state.terminal_queued {
            return Err(MailboxError::Closed);
        }
        state
            .queue
            .try_push_data_with(bytes, build)
            .map_err(map_queue)?;
        self.ready.notify_one();
        Ok(())
    }

    pub(super) fn try_push_control(
        &self,
        bytes: usize,
        deadline_ms: u64,
        build: impl FnOnce() -> ControlMessage,
    ) -> Result<(), MailboxError> {
        let mut state = self.lock();
        if state.closed {
            return Err(MailboxError::Closed);
        }
        state
            .queue
            .try_push_control_with(bytes, deadline_ms, build)
            .map_err(map_queue)?;
        self.ready.notify_one();
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
