use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::{background_terminals::TerminalAction, bindings::codex::request::OutgoingRequest};

/// Abandoned listings whose late answers are still dropped; past these, a
/// new listing waits for Codex to answer one of them.
const MAX_ABANDONED_LISTS: usize = 4;

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
        // A listing Codex never answers holds back only further listings, and
        // only once several are outstanding; a stop still goes out.
        let lists_held = matches!(action, TerminalAction::List)
            && self.terminals_tombstones.len() >= MAX_ABANDONED_LISTS;
        if self.terminals_pending() || lists_held {
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
    /// discarded by `take_terminals_tombstone`.
    pub(crate) fn abandon_terminals_list(&mut self) {
        if let Some(&(id, _)) = self
            .pending
            .iter()
            .find(|(_, kind)| *kind == RequestKind::TerminalsList)
        {
            self.take_pending(id);
            self.terminals_tombstones.push(id);
        }
    }

    pub(crate) fn take_terminals_tombstone(&mut self, id: u64) -> bool {
        let before = self.terminals_tombstones.len();
        self.terminals_tombstones
            .retain(|abandoned| *abandoned != id);
        self.terminals_tombstones.len() != before
    }
}
