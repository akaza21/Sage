use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::{background_terminals::TerminalAction, bindings::codex::request::OutgoingRequest};

impl CodexAdapter {
    /// Lists or stops the bound thread's background terminals, one request
    /// at a time and only once the session negotiated them.
    pub(crate) fn background_terminals(
        &mut self,
        id: u64,
        action: &TerminalAction,
    ) -> Result<OutgoingRequest, AdapterError> {
        if !self
            .negotiated
            .as_ref()
            .is_some_and(|methods| methods.snapshot.native_background_terminals)
        {
            return Err(AdapterError::CapabilityUnavailable);
        }
        if self.terminals_pending() || self.terminals_tombstone.is_some() {
            return Err(AdapterError::TurnIntentConflict);
        }
        let thread = self
            .external_thread_id
            .as_ref()
            .ok_or(AdapterError::SessionUnavailable)?;
        self.reserve(OutgoingRequest::background_terminals(id, thread, action)?)
    }

    pub(crate) fn terminals_pending(&self) -> bool {
        self.pending.iter().any(|(_, kind)| kind.is_terminals())
    }

    /// Stops waiting for a listing whose caller gave up. Its late answer is
    /// discarded, and no new request goes out until it arrives.
    pub(crate) fn abandon_terminals_list(&mut self) {
        if let Some(&(id, _)) = self
            .pending
            .iter()
            .find(|(_, kind)| *kind == RequestKind::TerminalsList)
        {
            self.take_pending(id);
            self.terminals_tombstone = Some(id);
        }
    }

    pub(crate) fn take_terminals_tombstone(&mut self, id: u64) -> bool {
        self.terminals_tombstone
            .take_if(|abandoned| *abandoned == id)
            .is_some()
    }
}
