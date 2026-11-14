use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::acp::store::{AcpStore, StoreError, StoreMutation};
use sage_core::limits::LIMITS;

use super::journal_support::{
    control_byte_limit, control_count_limit, data_byte_limit, data_count_limit, lane_name,
    mutation_retained_bytes,
};

const CONTROL_MESSAGE_BYTES: usize = 32 * 1024;

mod ack;
mod execution;
mod workspace_capture;
use ack::Acknowledgement;
pub(crate) use ack::JournalTicket;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalLane {
    Data,
    Control,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum JournalError {
    #[error("journal {lane} lane is at its bounded capacity")]
    Capacity { lane: &'static str },
    #[error("journal batch exceeds its transaction limit")]
    BatchLimit,
    #[error("journal actor is unavailable")]
    Closed,
    #[error("journal monotonic sequence is exhausted")]
    SequenceExhausted,
    #[error("journal entry expired before durable execution")]
    Deadline,
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, Clone)]
pub(crate) struct JournalHandle {
    queue: Arc<Queue>,
    workspace_database: Option<crate::persistence::Database>,
}

pub(crate) struct JournalRunner<S> {
    queue: Arc<Queue>,
    store: S,
}

pub(crate) struct JournalBatchBuilder {
    queue: Arc<Queue>,
    workspace_database: Option<crate::persistence::Database>,
    lane: JournalLane,
    mutations: Vec<StoreMutation>,
    reservation: Option<Reservation>,
}

#[derive(Debug)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

#[derive(Debug, Default)]
struct QueueState {
    closed: bool,
    aborted: bool,
    workspace_inflight: Vec<workspace_capture::InFlight>,
    next_sequence: u64,
    data_count: usize,
    data_bytes: usize,
    control_count: usize,
    control_bytes: usize,
    data: VecDeque<Envelope>,
    control: Vec<Envelope>,
    unknown_reservations: Vec<Reservation>,
}

#[derive(Debug)]
struct Envelope {
    workspace_database: Option<crate::persistence::Database>,
    workspace_result: Option<Result<(), StoreError>>,
    sequence: u64,
    deadline_millis: u64,
    priority_deadline_millis: u64,
    mutations: Arc<Vec<StoreMutation>>,
    reservation: Option<Reservation>,
    acknowledgement: Arc<Acknowledgement>,
}

#[derive(Debug)]
struct Reservation {
    queue: Arc<Queue>,
    lane: JournalLane,
    bytes: usize,
    released: bool,
}

impl JournalHandle {
    pub(crate) fn new<S: AcpStore>(store: S) -> (Self, JournalRunner<S>) {
        let queue = Arc::new(Queue {
            state: Mutex::new(QueueState {
                next_sequence: 1,
                ..QueueState::default()
            }),
            ready: Condvar::new(),
        });
        (
            Self {
                queue: Arc::clone(&queue),
                workspace_database: None,
            },
            JournalRunner { queue, store },
        )
    }

    pub(crate) fn with_workspace_tracking(
        mut self,
        database: crate::persistence::Database,
    ) -> Self {
        self.workspace_database = Some(database);
        self
    }

    /// Reserves the batch-count slot before the mutation vector receives its
    /// first entry. Each `push` reserves retained bytes before vector growth.
    pub(crate) fn batch(&self, lane: JournalLane) -> Result<JournalBatchBuilder, JournalError> {
        let reservation = Reservation::acquire(Arc::clone(&self.queue), lane)?;
        Ok(JournalBatchBuilder {
            queue: Arc::clone(&self.queue),
            workspace_database: self.workspace_database.clone(),
            lane,
            mutations: Vec::new(),
            reservation: Some(reservation),
        })
    }

    pub(crate) fn enqueue_one(
        &self,
        lane: JournalLane,
        mutation: StoreMutation,
        deadline_millis: u64,
    ) -> Result<JournalTicket, JournalError> {
        let mut batch = self.batch(lane)?;
        batch.push(mutation)?;
        batch.enqueue(deadline_millis)
    }

    pub(crate) fn shutdown(&self) {
        let (data, control) = {
            let mut state = lock(&self.queue);
            state.closed = true;
            state.aborted = true;
            for capture in &state.workspace_inflight {
                capture.acknowledgement.close();
            }
            (
                std::mem::take(&mut state.data),
                std::mem::take(&mut state.control),
            )
        };
        self.queue.ready.notify_all();
        for envelope in data.iter().chain(&control) {
            workspace_capture::discard(envelope);
            workspace_capture::resume(&mut lock(&self.queue), envelope);
        }
        drop(data);
        drop(control);
        self.queue.ready.notify_all();
    }

    pub(crate) fn close_when_drained(&self) {
        let mut state = lock(&self.queue);
        state.closed = true;
        drop(state);
        self.queue.ready.notify_all();
    }

    /// Called only after bounded authoritative recovery has resolved every
    /// commit-unknown outcome.
    pub(crate) fn release_unknown_reservations(&self) {
        let reservations = {
            let mut state = lock(&self.queue);
            std::mem::take(&mut state.unknown_reservations)
        };
        drop(reservations);
    }

    #[cfg(test)]
    pub(super) fn retained_counts(&self) -> (usize, usize) {
        let state = lock(&self.queue);
        (state.data_count, state.control_count)
    }

    #[cfg(all(test, feature = "acp-fixture"))]
    pub(crate) fn fixture_mutation_retained_bytes(mutation: &StoreMutation) -> usize {
        mutation_retained_bytes(mutation)
    }
}

impl JournalBatchBuilder {
    pub(crate) fn push(&mut self, mutation: StoreMutation) -> Result<(), JournalError> {
        if self.mutations.len() >= LIMITS.queues.journal_facts_per_transaction as usize {
            return Err(JournalError::BatchLimit);
        }
        let bytes = mutation_retained_bytes(&mutation);
        let current = self.reservation.as_ref().ok_or(JournalError::Closed)?.bytes;
        if current
            .checked_add(bytes)
            .is_none_or(|next| next > LIMITS.queues.journal_bytes_per_transaction as usize)
            || (self.lane == JournalLane::Control && current + bytes > CONTROL_MESSAGE_BYTES)
        {
            return Err(JournalError::BatchLimit);
        }
        self.reservation
            .as_mut()
            .ok_or(JournalError::Closed)?
            .reserve_bytes(bytes)?;
        self.mutations.push(mutation);
        Ok(())
    }

    pub(crate) fn enqueue(mut self, deadline_millis: u64) -> Result<JournalTicket, JournalError> {
        if self.mutations.is_empty() {
            return Err(JournalError::BatchLimit);
        }
        let reservation = self.reservation.take().ok_or(JournalError::Closed)?;
        let mutations = std::mem::take(&mut self.mutations);
        let acknowledgement = Arc::new(Acknowledgement::new());
        let now_millis = now_millis();
        let deadline_millis = if deadline_millis == 0 {
            now_millis.saturating_add(LIMITS.deadlines_ms.ordinary_rpc)
        } else {
            deadline_millis
        };
        let priority_deadline_millis = monotonic_deadline_millis(Duration::from_millis(
            deadline_millis.saturating_sub(now_millis),
        ));
        let mut state = lock(&self.queue);
        if state.closed {
            drop(state);
            drop(reservation);
            return Err(JournalError::Closed);
        }
        let sequence = state.next_sequence;
        state.next_sequence = state
            .next_sequence
            .checked_add(1)
            .filter(|next| *next <= i64::MAX as u64)
            .ok_or(JournalError::SequenceExhausted)?;
        let envelope = Envelope {
            workspace_database: self.workspace_database.clone(),
            workspace_result: None,
            sequence,
            deadline_millis,
            priority_deadline_millis,
            mutations: Arc::new(mutations),
            reservation: Some(reservation),
            acknowledgement: Arc::clone(&acknowledgement),
        };
        match self.lane {
            JournalLane::Data => state.data.push_back(envelope),
            JournalLane::Control => state.control.push(envelope),
        }
        drop(state);
        self.queue.ready.notify_one();
        Ok(JournalTicket::new(acknowledgement, deadline_millis))
    }
}

impl<S: AcpStore + 'static> JournalRunner<S> {
    #[cfg(test)]
    pub(crate) fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_blocking(move || self.run_blocking())
    }

    pub(crate) fn run_blocking(self) {
        self.run_with_workspace_capture(Arc::new(crate::turn_workspace::prepare_terminal));
    }

    fn run_with_workspace_capture(mut self, prepare: Arc<workspace_capture::Prepare>) {
        loop {
            let Some(envelope) = next_envelope(&self.queue) else {
                break;
            };
            execution::execute_envelope(&self.queue, &mut self.store, envelope, &prepare);
        }
    }
}

impl Drop for Envelope {
    fn drop(&mut self) {
        self.acknowledgement.close();
    }
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

fn monotonic_deadline_millis(after: Duration) -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_millis();
    elapsed
        .saturating_add(after.as_millis())
        .min(u128::from(u64::MAX)) as u64
}

impl Reservation {
    fn acquire(queue: Arc<Queue>, lane: JournalLane) -> Result<Self, JournalError> {
        let mut state = lock(&queue);
        if state.closed {
            return Err(JournalError::Closed);
        }
        let (count, limit) = match lane {
            JournalLane::Data => (state.data_count, data_count_limit()),
            JournalLane::Control => (state.control_count, control_count_limit()),
        };
        if count >= limit {
            return Err(JournalError::Capacity {
                lane: lane_name(lane),
            });
        }
        match lane {
            JournalLane::Data => state.data_count += 1,
            JournalLane::Control => state.control_count += 1,
        }
        drop(state);
        Ok(Self {
            queue,
            lane,
            bytes: 0,
            released: false,
        })
    }

    fn reserve_bytes(&mut self, added: usize) -> Result<(), JournalError> {
        let mut state = lock(&self.queue);
        let (retained, limit) = match self.lane {
            JournalLane::Data => (state.data_bytes, data_byte_limit()),
            JournalLane::Control => (state.control_bytes, control_byte_limit()),
        };
        if retained.checked_add(added).is_none_or(|next| next > limit) {
            return Err(JournalError::Capacity {
                lane: lane_name(self.lane),
            });
        }
        match self.lane {
            JournalLane::Data => state.data_bytes += added,
            JournalLane::Control => state.control_bytes += added,
        }
        self.bytes += added;
        Ok(())
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut state = lock(&self.queue);
        match self.lane {
            JournalLane::Data => {
                state.data_count = state.data_count.saturating_sub(1);
                state.data_bytes = state.data_bytes.saturating_sub(self.bytes);
            }
            JournalLane::Control => {
                state.control_count = state.control_count.saturating_sub(1);
                state.control_bytes = state.control_bytes.saturating_sub(self.bytes);
            }
        }
        self.released = true;
    }
}

fn next_envelope(queue: &Queue) -> Option<Envelope> {
    let mut state = lock(queue);
    loop {
        if let Some(index) = state
            .control
            .iter()
            .enumerate()
            .filter(|(_, envelope)| !workspace_capture::blocked(&state, envelope))
            .min_by_key(|(_, envelope)| (envelope.priority_deadline_millis, envelope.sequence))
            .map(|(index, _)| index)
        {
            let envelope = state.control.swap_remove(index);
            workspace_capture::resume(&mut state, &envelope);
            return Some(envelope);
        }
        if let Some(index) = state
            .data
            .iter()
            .position(|envelope| !workspace_capture::blocked(&state, envelope))
        {
            let envelope = state.data.remove(index).expect("selected data envelope");
            workspace_capture::resume(&mut state, &envelope);
            return Some(envelope);
        }
        if state.aborted || (state.closed && state.workspace_inflight.is_empty()) {
            return None;
        }
        state = queue
            .ready
            .wait(state)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

fn lock(queue: &Queue) -> std::sync::MutexGuard<'_, QueueState> {
    queue
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
