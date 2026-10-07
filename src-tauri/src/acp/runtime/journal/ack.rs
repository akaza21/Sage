use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use tokio::sync::Notify;

use crate::acp::store::{StoreCommit, StoreError};

use super::{JournalError, now_millis};

pub(crate) struct JournalTicket {
    acknowledgement: Arc<Acknowledgement>,
    started: Instant,
    timeout: Duration,
}

#[derive(Debug)]
pub(super) struct Acknowledgement {
    state: Mutex<State>,
    blocking_ready: Condvar,
    async_ready: Notify,
}

#[derive(Debug)]
enum State {
    Pending,
    Executing,
    Completed(Result<StoreCommit, JournalError>),
    TimedOutKnown,
    TimedOutUnknown,
    Closed,
}

impl JournalTicket {
    pub(super) fn new(acknowledgement: Arc<Acknowledgement>, deadline_millis: u64) -> Self {
        Self {
            acknowledgement,
            started: Instant::now(),
            timeout: Duration::from_millis(deadline_millis.saturating_sub(now_millis())),
        }
    }

    pub(crate) async fn committed(self) -> Result<StoreCommit, JournalError> {
        loop {
            let notified = self.acknowledgement.async_ready.notified();
            if let Some(result) = self.acknowledgement.take_completed()? {
                return result;
            }
            let Ok(remaining) = self.remaining() else {
                return self.acknowledgement.timeout();
            };
            if tokio::time::timeout(remaining, notified).await.is_err() {
                return self.acknowledgement.timeout();
            }
        }
    }

    pub(crate) fn blocking_committed(self) -> Result<StoreCommit, JournalError> {
        let mut state = self.acknowledgement.lock();
        loop {
            match &*state {
                State::Completed(_) => return take_result(&mut state),
                State::Closed => return Err(JournalError::Closed),
                State::TimedOutKnown => return Err(JournalError::Deadline),
                State::TimedOutUnknown => return commit_unknown(),
                State::Pending | State::Executing => {}
            }
            let Ok(remaining) = self.remaining() else {
                drop(state);
                return self.acknowledgement.timeout();
            };
            let (next, timed) = self
                .acknowledgement
                .blocking_ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            if timed.timed_out() && matches!(*state, State::Pending | State::Executing) {
                drop(state);
                return self.acknowledgement.timeout();
            }
        }
    }

    fn remaining(&self) -> Result<Duration, JournalError> {
        let remaining = self.timeout.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            Err(JournalError::Deadline)
        } else {
            Ok(remaining)
        }
    }

    #[cfg(test)]
    pub(crate) fn expire_deadline_for_test(&mut self) {
        self.timeout = Duration::ZERO;
    }
}

impl Drop for JournalTicket {
    fn drop(&mut self) {
        self.acknowledgement.abandon();
    }
}

impl Acknowledgement {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(State::Pending),
            blocking_ready: Condvar::new(),
            async_ready: Notify::new(),
        }
    }

    pub(super) fn begin_execution(&self) -> bool {
        let mut state = self.lock();
        if matches!(*state, State::Pending) {
            *state = State::Executing;
            true
        } else {
            false
        }
    }

    /// Returns true when the caller already timed out or abandoned the ticket,
    /// making the commit outcome unknown at the API boundary.
    pub(super) fn complete(&self, result: Result<StoreCommit, JournalError>) -> bool {
        let mut state = self.lock();
        let unknown = matches!(*state, State::TimedOutUnknown);
        if matches!(*state, State::Pending | State::Executing) {
            *state = State::Completed(result);
        }
        drop(state);
        self.blocking_ready.notify_all();
        self.async_ready.notify_one();
        unknown
    }

    pub(super) fn close(&self) {
        let mut state = self.lock();
        if matches!(*state, State::Pending) {
            *state = State::Closed;
        }
        drop(state);
        self.blocking_ready.notify_all();
        self.async_ready.notify_one();
    }

    fn take_completed(&self) -> Result<Option<Result<StoreCommit, JournalError>>, JournalError> {
        let mut state = self.lock();
        match &*state {
            State::Pending | State::Executing => Ok(None),
            State::TimedOutKnown => Err(JournalError::Deadline),
            State::TimedOutUnknown => Err(StoreError::CommitUnknown.into()),
            State::Closed => Err(JournalError::Closed),
            State::Completed(_) => Ok(Some(take_result(&mut state))),
        }
    }

    fn timeout(&self) -> Result<StoreCommit, JournalError> {
        let mut state = self.lock();
        match &*state {
            State::Pending => {
                *state = State::TimedOutKnown;
                Err(JournalError::Deadline)
            }
            State::Executing => {
                *state = State::TimedOutUnknown;
                commit_unknown()
            }
            State::TimedOutKnown => Err(JournalError::Deadline),
            State::TimedOutUnknown => commit_unknown(),
            State::Closed => Err(JournalError::Closed),
            State::Completed(_) => take_result(&mut state),
        }
    }

    fn abandon(&self) {
        let mut state = self.lock();
        match *state {
            State::Pending => *state = State::TimedOutKnown,
            State::Executing => *state = State::TimedOutUnknown,
            State::Completed(_) | State::TimedOutKnown | State::TimedOutUnknown | State::Closed => {
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn take_result(state: &mut State) -> Result<StoreCommit, JournalError> {
    let State::Completed(result) = std::mem::replace(state, State::Closed) else {
        unreachable!("completed acknowledgement retained")
    };
    result
}

fn commit_unknown<T>() -> Result<T, JournalError> {
    Err(StoreError::CommitUnknown.into())
}
