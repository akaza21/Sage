use super::{AdapterError, CodexAdapter};
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
        if self.terminals_pending() {
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
}
