use std::time::Instant;

use super::Actor;
use super::super::types::{HostError, HostOutcome, HostReply, HostState, RuntimeEngine};

impl<E: RuntimeEngine> Actor<E> {
    pub(super) fn accept_durable_completed(&mut self) -> Result<HostReply, HostError> {
        if self.state != HostState::Running {
            return Ok(HostReply::Outcome(HostOutcome::TurnTerminal));
        }
        match self.engine.accept_latest_completed() {
            Ok(()) => {
                self.active_turn = None;
                self.pending_cancel = None;
                self.pending_terminal = None;
                self.state = HostState::Idle;
                Ok(HostReply::Outcome(HostOutcome::TurnTerminal))
            }
            Err(HostError::Closed | HostError::State | HostError::Intent) => {
                Ok(HostReply::Outcome(HostOutcome::TurnTerminal))
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn begin_park(&mut self) -> Result<HostReply, HostError> {
        match self.engine.begin_park() {
            Ok(()) => Ok(HostReply::Outcome(HostOutcome::Shutdown)),
            Err(error) => Err(error),
        }
    }

    pub(super) fn finish_park(&mut self, deadline: Instant) -> Result<HostReply, HostError> {
        match self.engine.park_zero(deadline) {
            Ok(()) => Ok(HostReply::Outcome(HostOutcome::Shutdown)),
            Err(error) => Err(error),
        }
    }

    pub(super) fn shutdown(&mut self, deadline: Instant) -> Result<HostReply, HostError> {
        self.state = HostState::Draining;
        match self.engine.shutdown(deadline) {
            Ok(()) => {
                self.owner = None;
                self.active_turn = None;
                self.pending_prompt = false;
                self.pending_cancel = None;
                self.pending_terminal = None;
                self.pending_close = None;
                self.state = HostState::Closed;
                Ok(HostReply::Outcome(HostOutcome::Shutdown))
            }
            Err(error) => {
                self.state = HostState::Recovery;
                Err(error)
            }
        }
    }
}
