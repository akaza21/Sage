//! A Turn Codex starts on its own while Sage runs none on the bound thread,
//! such as an active goal's continuation after `thread/resume` or after a
//! Turn ends. It is announced to the runtime, which opens it as a
//! harness-started Turn and adopts it here under that Turn's client message
//! id, so its items, end, Stop and errors take the prompted Turn's path.
//! Codex has no decline: while a Turn is announced no prompt is admitted, so
//! a Turn Sage refuses ends with the runtime, never bound to a prompt.
//!
//! Correlation is left to Codex. A `turn/started` while a Sage `turn/start`
//! is unanswered stays with that request: Codex answers a `turn/start` sent
//! during its own Turn with that Turn's id (it takes the prompt into it), and
//! a reply naming another Turn stays an identity conflict.
use sage_acp_protocol::{
    access_mode::SessionAccessMode, contract::RequiredText, intent::ExternalId,
};

use super::super::{AdapterError, CodexAdapter, ExternalKind, ScopedExternalId};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::acp::bindings::codex) struct NativeTurn {
    announced: Option<ExternalId>,
    /// `thread/start` and `thread/resume` leave the thread read-only; each
    /// prompt whose `turn/start` Codex accepts sets the policy the thread's
    /// later Turns keep.
    thread_access: SessionAccessMode,
    /// A sent `turn/start` and the access it asks for, until Codex answers.
    pending_access: Option<(u64, SessionAccessMode)>,
}

impl Default for NativeTurn {
    fn default() -> Self {
        Self {
            announced: None,
            thread_access: SessionAccessMode::ReadOnly,
            pending_access: None,
        }
    }
}

impl CodexAdapter {
    /// The session identity that announces `turn`, when no Sage request
    /// started, awaits or ended it and no Turn runs. `None` leaves the
    /// `turn/started` to correlation, which rejects what it cannot place.
    pub(crate) fn announce_native_turn(
        &mut self,
        turn: &str,
    ) -> Result<Option<ScopedExternalId>, AdapterError> {
        let id = ExternalId::new("externalTurnId", turn.to_owned())?;
        if self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
            || self.action_handoff.is_some()
            || self.turn_start_correlations.has_pending()
            || self.turn_start_correlations.knows(&id)
            || self
                .previous_turn
                .as_ref()
                .is_some_and(|previous| previous.id == id)
        {
            return Ok(None);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        if self.native_turn.announced.replace(id).is_some() {
            return Err(AdapterError::IdentityConflict);
        }
        Ok(Some(self.scoped(ExternalKind::Session, thread)))
    }

    pub(crate) fn has_announced_native_turn(&self) -> bool {
        self.native_turn.announced.is_some()
    }

    /// The access the announced Turn runs under.
    pub(crate) fn native_turn_access(&self) -> Option<SessionAccessMode> {
        self.native_turn
            .announced
            .as_ref()
            .map(|_| self.native_turn.thread_access)
    }

    pub(in crate::acp::bindings::codex::state) fn ask_thread_access(
        &mut self,
        request_id: u64,
        access: SessionAccessMode,
    ) {
        self.native_turn.pending_access = Some((request_id, access));
    }

    /// Codex accepted the `turn/start` `request_id`, and with it its policy.
    pub(crate) fn accept_thread_access(&mut self, request_id: u64) {
        if let Some((pending, access)) = self.native_turn.pending_access
            && pending == request_id
        {
            self.native_turn.thread_access = access;
            self.native_turn.pending_access = None;
        }
    }

    /// Makes the announced Turn the active one, owned by the Turn Sage
    /// opened for it.
    pub(crate) fn adopt_native_turn(
        &mut self,
        client_message_id: RequiredText<512>,
    ) -> Result<ScopedExternalId, AdapterError> {
        let id = self
            .native_turn
            .announced
            .take()
            .ok_or(AdapterError::TurnIntentConflict)?;
        if self.active_turn_id.is_some() || self.provisional_turn.is_some() {
            return Err(AdapterError::TurnIntentConflict);
        }
        self.active_turn_client_message_id = Some(client_message_id);
        self.active_item_terminals.clear();
        self.active_unretried_error_fingerprints.clear();
        self.active_terminal_id_bytes = 0;
        self.active_turn_id = Some(id.clone());
        self.scoped_turn(id.as_str())
    }
}
