use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::bindings::codex::request::OutgoingRequest;

/// Reads whose callers gave up but which Codex may still answer.
const MAX_ABANDONED_READS: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct AbandonedReads {
    ids: Vec<u64>,
    /// No fresh data arrived since the last abandon, so a new read would
    /// only queue behind it.
    blocking: bool,
}

impl CodexAdapter {
    pub(crate) fn rate_limits_read(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        if self.negotiated.is_none() {
            return Err(AdapterError::CapabilityUnavailable);
        }
        let abandoned = &self.rate_limits_abandoned;
        if self.rate_limits_read_pending()
            || abandoned.blocking
            || abandoned.ids.len() >= MAX_ABANDONED_READS
        {
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
            self.rate_limits_abandoned.ids.push(id);
            self.rate_limits_abandoned.blocking = true;
        }
    }

    /// A pushed snapshot shows Codex is answering again.
    pub(crate) fn rate_limits_pushed(&mut self) {
        self.rate_limits_abandoned.blocking = false;
    }

    pub(crate) fn take_rate_limits_tombstone(&mut self, id: u64) -> bool {
        let abandoned = &mut self.rate_limits_abandoned;
        let Some(index) = abandoned.ids.iter().position(|abandoned| *abandoned == id) else {
            return false;
        };
        abandoned.ids.swap_remove(index);
        abandoned.blocking &= !abandoned.ids.is_empty();
        true
    }

    fn rate_limits_read_id(&self) -> Option<u64> {
        self.pending
            .iter()
            .find(|(_, kind)| *kind == RequestKind::RateLimitsRead)
            .map(|(id, _)| *id)
    }
}
