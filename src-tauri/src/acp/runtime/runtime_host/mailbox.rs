use std::{
    sync::{Condvar, Mutex, OnceLock, mpsc::SyncSender},
    time::{Duration, Instant},
};

use crate::acp::{limits::LIMITS, transport::ReservedQueue};

use super::types::{
    ConfigCompletion, HostCommand, HostError, HostReply, NativeRpcCompletion, SteerCompletion,
};

pub(super) struct Envelope {
    pub(super) command: HostCommand,
    pub(super) reply: SyncSender<Result<HostReply, HostError>>,
    pub(super) deadline: Instant,
}

struct State {
    queue: ReservedQueue<Envelope>,
    config_completion: Option<ConfigCompletion>,
    native_rpc_completion: Option<NativeRpcCompletion>,
    steer_completion: Option<SteerCompletion>,
    closed: bool,
}

pub(super) enum HostMessage {
    Command(Envelope),
    ConfigCompletion(ConfigCompletion),
    NativeRpcCompletion(NativeRpcCompletion),
    SteerCompletion(SteerCompletion),
    OperationDeadline,
}

pub(super) struct HostMailbox {
    state: Mutex<State>,
    ready: Condvar,
}

impl HostMailbox {
    pub(super) fn new() -> Result<Self, HostError> {
        let queue = ReservedQueue::new(
            LIMITS.queues.runtime_tasks_per_session as usize,
            LIMITS.queues.journal_control_bytes_inside_total as usize,
            LIMITS.queues.egress_control_frames_inside_total as usize,
            LIMITS.queues.egress_control_bytes_inside_total as usize / 2,
        )
        .map_err(|_| HostError::Capacity)?;
        Ok(Self {
            state: Mutex::new(State {
                queue,
                config_completion: None,
                native_rpc_completion: None,
                steer_completion: None,
                closed: false,
            }),
            ready: Condvar::new(),
        })
    }

    pub(super) fn try_push(&self, envelope: Envelope) -> Result<(), HostError> {
        if Instant::now() >= envelope.deadline {
            return Err(HostError::Deadline);
        }
        let bytes = envelope.command.actual_bytes();
        let control = envelope.command.is_control();
        let deadline_millis =
            monotonic_deadline_millis(envelope.deadline.saturating_duration_since(Instant::now()));
        let mut retained = Some(envelope);
        let mut state = self.lock();
        if state.closed {
            return Err(HostError::Closed);
        }
        let result = if control {
            state
                .queue
                .try_push_control_with(bytes, deadline_millis, || {
                    retained.take().expect("host envelope retained")
                })
        } else {
            state
                .queue
                .try_push_data_with(bytes, || retained.take().expect("host envelope retained"))
        };
        result.map_err(|_| HostError::Capacity)?;
        self.ready.notify_one();
        Ok(())
    }

    pub(super) fn complete_config(&self, completion: ConfigCompletion) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        let replace = state
            .config_completion
            .as_ref()
            .is_none_or(|retained| completion.operation_id > retained.operation_id);
        if replace {
            state.config_completion = Some(completion);
            self.ready.notify_one();
        }
    }

    pub(super) fn complete_native_rpc(&self, completion: NativeRpcCompletion) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        let replace = state
            .native_rpc_completion
            .as_ref()
            .is_none_or(|retained| completion.operation_id > retained.operation_id);
        if replace {
            state.native_rpc_completion = Some(completion);
            self.ready.notify_one();
        }
    }

    pub(super) fn complete_steer(&self, completion: SteerCompletion) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        let replace = state
            .steer_completion
            .as_ref()
            .is_none_or(|retained| completion.operation_id > retained.operation_id);
        if replace {
            state.steer_completion = Some(completion);
            self.ready.notify_one();
        }
    }

    pub(super) fn wait_next(&self, operation_deadline: Option<Instant>) -> Option<HostMessage> {
        loop {
            let expired = {
                let mut state = self.lock();
                loop {
                    if let Some(completion) = state.config_completion.take() {
                        return Some(HostMessage::ConfigCompletion(completion));
                    }
                    if let Some(completion) = state.native_rpc_completion.take() {
                        return Some(HostMessage::NativeRpcCompletion(completion));
                    }
                    if let Some(completion) = state.steer_completion.take() {
                        return Some(HostMessage::SteerCompletion(completion));
                    }
                    match state.queue.pop() {
                        Some(envelope) if Instant::now() >= envelope.deadline => {
                            // Never dispatch work that expired while queued.
                            break Some(envelope);
                        }
                        Some(envelope) => return Some(HostMessage::Command(envelope)),
                        None if state.closed => return None,
                        None => match operation_deadline {
                            Some(deadline) => {
                                let remaining = deadline.saturating_duration_since(Instant::now());
                                if remaining.is_zero() {
                                    return Some(HostMessage::OperationDeadline);
                                }
                                let (next, timeout) = self
                                    .ready
                                    .wait_timeout(state, remaining)
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                state = next;
                                if timeout.timed_out()
                                    && state.config_completion.is_none()
                                    && state.steer_completion.is_none()
                                    && state.native_rpc_completion.is_none()
                                    && state.queue.data_usage().0 == 0
                                    && state.queue.control_usage().0 == 0
                                {
                                    return Some(HostMessage::OperationDeadline);
                                }
                            }
                            None => {
                                state = self
                                    .ready
                                    .wait(state)
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                            }
                        },
                    }
                }
            };
            if let Some(envelope) = expired {
                let _ = envelope.reply.try_send(Err(HostError::Deadline));
            }
        }
    }

    pub(super) fn close(&self) {
        let drained = {
            let mut state = self.lock();
            state.closed = true;
            state.config_completion = None;
            state.native_rpc_completion = None;
            let mut drained = Vec::new();
            while let Some(envelope) = state.queue.pop() {
                drained.push(envelope);
            }
            self.ready.notify_all();
            drained
        };
        // A bounded reply can itself be back-pressured; no mailbox mutex is
        // held while completing callers.
        for envelope in drained {
            let _ = envelope.reply.try_send(Err(HostError::Closed));
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn monotonic_deadline_millis(after: Duration) -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_millis();
    elapsed
        .saturating_add(after.as_millis())
        .min(u128::from(u64::MAX)) as u64
}
