use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::bindings::codex::request::OutgoingRequest;

impl CodexAdapter {
    pub(crate) fn rate_limits_read(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        if self.negotiated.is_none() {
            return Err(AdapterError::CapabilityUnavailable);
        }
        if self.rate_limits_read_pending() {
            return Err(AdapterError::TurnIntentConflict);
        }
        self.reserve(OutgoingRequest::rate_limits_read(id)?)
    }

    pub(crate) fn rate_limits_read_pending(&self) -> bool {
        self.rate_limits_read_id().is_some()
    }

    /// Stops waiting for a read whose caller gave up; its late answer is
    /// discarded by `take_rate_limits_tombstone`.
    pub(crate) fn abandon_rate_limits_read(&mut self) {
        if let Some(id) = self.rate_limits_read_id() {
            self.take_pending(id);
            self.rate_limits_tombstone = Some(id);
        }
    }

    pub(crate) fn take_rate_limits_tombstone(&mut self, id: u64) -> bool {
        let abandoned = self.rate_limits_tombstone == Some(id);
        if abandoned {
            self.rate_limits_tombstone = None;
        }
        abandoned
    }

    fn rate_limits_read_id(&self) -> Option<u64> {
        self.pending
            .iter()
            .find(|(_, kind)| *kind == RequestKind::RateLimitsRead)
            .map(|(id, _)| *id)
    }
}
