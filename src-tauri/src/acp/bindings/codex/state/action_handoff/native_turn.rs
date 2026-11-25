//! A Turn Codex starts on its own while Sage runs none on the bound thread,
//! such as an active goal's continuation after `thread/resume`. It is
//! announced to the runtime, which opens it as a harness-started Turn and
//! then adopts it here under that Turn's client message id, so its items,
//! end, Stop and errors take the prompted Turn's path. Correlation is left
//! to Codex: a `turn/started` while a Sage turn start is unanswered stays
//! with that request, since Codex answers a `turn/start` sent during its own
//! Turn with that Turn's id.
use sage_acp_protocol::{
    access_mode::SessionAccessMode, contract::RequiredText, intent::ExternalId,
};

use super::super::{AdapterError, CodexAdapter, ExternalKind, ScopedExternalId};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::acp::bindings::codex) struct NativeTurn {
    announced: Option<ExternalId>,
    /// `thread/start` and `thread/resume` leave the thread read-only; each
    /// prompt's `turn/start` sets the policy the thread's later Turns keep.
    thread_access: SessionAccessMode,
}

impl Default for NativeTurn {
    fn default() -> Self {
        Self {
            announced: None,
            thread_access: SessionAccessMode::ReadOnly,
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

    /// The access the announced Turn runs under.
    pub(crate) fn native_turn_access(&self) -> Option<SessionAccessMode> {
        self.native_turn
            .announced
            .as_ref()
            .map(|_| self.native_turn.thread_access)
    }

    pub(in crate::acp::bindings::codex::state) fn record_thread_access(
        &mut self,
        access: SessionAccessMode,
    ) {
        self.native_turn.thread_access = access;
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
