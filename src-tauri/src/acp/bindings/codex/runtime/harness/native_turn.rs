//! Opens a Turn Codex started on its own as a harness-started Turn; see
//! `state::action_handoff::native_turn`. Codex has no decline: a Turn Sage
//! refuses fails the runtime closed, after the pump shows why.
use crate::acp::bindings::adapter::{AdapterBatch, AdapterRuntimeError, HarnessTurnAuthority};
use sage_acp_protocol::{contract::RequiredText, lifecycle::TurnState};

use super::super::{CodexRuntime, IncomingEvent, IngressDisposition, support::map_adapter};

impl CodexRuntime {
    /// The announced Turn, now running under the Turn Sage opened for it.
    pub(super) fn adopt_native_turn(
        &mut self,
        client_message_id: RequiredText<512>,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let id = self
            .inner
            .adopt_native_turn(client_message_id)
            .map_err(map_adapter)?;
        let running = IncomingEvent::Turn {
            id,
            state: TurnState::Running,
        };
        // The request digest identifies permission requests; a Turn has none.
        self.translate(IngressDisposition::Events(vec![running]), [0; 32])
    }

    /// The announced Turn runs under the access Sage last gave the thread.
    pub(super) fn native_turn_authority(&self) -> Option<HarnessTurnAuthority> {
        self.inner
            .native_turn_access()
            .map(HarnessTurnAuthority::Access)
    }
}
