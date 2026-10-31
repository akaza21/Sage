use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use super::host::{
    HOST_AWAITING_EXIT, HOST_JOINED, HOST_RUNNING, HOST_SHUTDOWN_ACKNOWLEDGED, RuntimeHost,
};
use super::types::{HostCommand, HostError, HostOutcome};

impl RuntimeHost {
    pub(crate) fn close_deadline() -> Instant {
        Instant::now()
            + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close)
    }

    pub(crate) fn shutdown(&self, deadline: Instant) -> Result<(), HostError> {
        // A timed-out actor has already closed its mailbox and is only waiting
        // to exit. Do not submit further lifecycle commands to that closed
        // authority; join the retained actor within the caller's deadline.
        if self.lifecycle.load(Ordering::Acquire) != HOST_RUNNING {
            return self.shutdown_before(deadline);
        }
        self.record_shutdown_phase("drain");
        match self.submit_with_timeout(
            HostCommand::State,
            deadline.saturating_duration_since(Instant::now()),
        ) {
            Ok(_) | Err(HostError::Closed | HostError::State | HostError::Intent) => {}
            Err(error) => return Err(error),
        }
        self.record_shutdown_phase("accept");
        match self.outcome_with_timeout(
            HostCommand::AcceptDurableCompleted,
            deadline.saturating_duration_since(Instant::now()),
        ) {
            Ok(_) | Err(HostError::Closed | HostError::State | HostError::Intent) => {}
            Err(error) => return Err(error),
        }
        self.record_shutdown_phase("begin_park");
        match self.outcome_with_timeout(
            HostCommand::BeginPark,
            deadline.saturating_duration_since(Instant::now()),
        ) {
            Ok(_) | Err(HostError::Closed | HostError::State) => {}
            Err(HostError::Intent) => {
                self.record_shutdown_phase("begin_park_intent");
                return self.shutdown_before(deadline);
            }
            Err(error) => return Err(error),
        }
        self.record_shutdown_phase("finish_park");
        match self.outcome_with_timeout(
            HostCommand::FinishPark,
            deadline.saturating_duration_since(Instant::now()),
        ) {
            Ok(_) | Err(HostError::Closed | HostError::State) => {}
            Err(error) => return Err(error),
        }
        self.record_shutdown_phase("join");
        self.shutdown_before(deadline)
    }

    fn record_shutdown_phase(&self, _phase: &'static str) {
        #[cfg(test)]
        {
            *self
                .last_shutdown_phase
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = _phase;
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn last_shutdown_phase(&self) -> &'static str {
        *self
            .last_shutdown_phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn shutdown_before(&self, deadline: Instant) -> Result<(), HostError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.lifecycle.load(Ordering::Acquire) {
            HOST_JOINED => return Ok(()),
            HOST_AWAITING_EXIT | HOST_SHUTDOWN_ACKNOWLEDGED => {
                return self.join_with_timeout(remaining);
            }
            HOST_RUNNING => {}
            _ => return Err(HostError::RecoveryRequired),
        }
        match self.outcome_with_timeout(HostCommand::Shutdown, remaining)? {
            HostOutcome::Shutdown => {
                self.lifecycle
                    .store(HOST_SHUTDOWN_ACKNOWLEDGED, Ordering::Release);
                self.join_with_timeout(deadline.saturating_duration_since(Instant::now()))
            }
            _ => Err(HostError::State),
        }
    }
}
