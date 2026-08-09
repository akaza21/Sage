use std::{
    sync::{Condvar, Mutex, OnceLock, mpsc::SyncSender},
    time::{Duration, Instant},
};

use crate::acp::{limits::LIMITS, transport::ReservedQueue};

use super::types::{HostCommand, HostError, HostReply};

pub(super) struct Envelope {
    pub(super) command: HostCommand,
    pub(super) reply: SyncSender<Result<HostReply, HostError>>,
    pub(super) deadline: Instant,
}

struct State {
    queue: ReservedQueue<Envelope>,
    closed: bool,
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
                closed: false,
            }),
            ready: Condvar::new(),
        })
    }

    pub(super) fn try_push(&self, envelope: Envelope) -> Result<(), HostError> {
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

    pub(super) fn wait_next(&self) -> Option<Envelope> {
        let mut state = self.lock();
        loop {
            if let Some(envelope) = state.queue.pop() {
                return Some(envelope);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    pub(super) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        while let Some(envelope) = state.queue.pop() {
            let _ = envelope.reply.send(Err(HostError::Closed));
        }
        self.ready.notify_all();
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
