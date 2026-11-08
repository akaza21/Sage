//! A session action Codex answers before it names the action's Turn:
//! `thread/compact/start` answers `{}` and `turn/started` follows. Until
//! then the Turn is held like a provisional one, with the same deadline.
use std::time::{Duration, Instant};

use crate::acp::bindings::adapter::PROVISIONAL_TURN_HANDOFF_MILLIS;

use super::{AdapterError, CodexAdapter, RequestKind, TurnStartFailure};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ActionHandoff {
    request_id: u64,
    deadline: Instant,
}

impl CodexAdapter {
    pub(in crate::acp::bindings::codex) fn accept_compact_response(
        &mut self,
        id: u64,
    ) -> Result<(), AdapterError> {
        if self.pending_kind(id) != Some(RequestKind::Compact) {
            return Err(AdapterError::TurnIntentConflict);
        }
        if !self.turn_start_correlations.is_pending(id) {
            return self.finish_turn_start_response(id);
        }
        if self.action_handoff.is_some() {
            return Err(AdapterError::TurnIntentConflict);
        }
        self.take_pending(id);
        self.action_handoff = Some(ActionHandoff {
            request_id: id,
            deadline: Instant::now() + Duration::from_millis(PROVISIONAL_TURN_HANDOFF_MILLIS),
        });
        Ok(())
    }

    /// `turn/started` named the held action's Turn.
    pub(super) fn retire_action_handoff(&mut self) -> Result<(), AdapterError> {
        match self.action_handoff.take() {
            Some(handoff) => self.turn_start_correlations.consume(handoff.request_id),
            None => Ok(()),
        }
    }

    /// Codex failed the thread before it named the held action's Turn.
    pub(super) fn fail_action_handoff(&mut self) -> Result<TurnStartFailure, AdapterError> {
        let handoff = self
            .action_handoff
            .take()
            .ok_or(AdapterError::TurnIntentConflict)?;
        let failure = self.fail_correlation(handoff.request_id)?;
        self.turn_start_correlations.consume(handoff.request_id)?;
        Ok(failure)
    }

    pub(crate) fn has_action_handoff(&self) -> bool {
        self.action_handoff.is_some()
    }

    pub(super) fn action_handoff_expired(&self) -> bool {
        self.action_handoff
            .as_ref()
            .is_some_and(|handoff| Instant::now() >= handoff.deadline)
    }

    /// A Turn is admitted but not yet named, so `turn/interrupt` cannot
    /// address it: hold the cancel until the Turn binds.
    pub(crate) fn defer_cancel(&mut self) -> bool {
        if self.active_turn_id.is_some() || !self.turn_start_correlations.has_pending() {
            return false;
        }
        self.deferred_interrupt = true;
        true
    }

    /// True once, when a held cancel's Turn is bound and can be interrupted.
    pub(crate) fn take_deferred_interrupt(&mut self) -> bool {
        if !self.deferred_interrupt || self.active_turn_id.is_none() {
            return false;
        }
        self.deferred_interrupt = false;
        true
    }
}
