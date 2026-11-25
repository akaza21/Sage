//! Opens a Turn Codex started on its own as a harness-started Turn; see
//! `state::action_handoff::native_turn`.
use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterEgress, AdapterRuntimeError, EgressToken, HarnessTurnAuthority,
    },
    transport::OutboundFrame,
};
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

    /// Interrupts the announced Turn Sage refused. The interrupt stops what
    /// Codex started on its own, so an idle runtime may write it.
    pub(super) fn decline_native_turn(
        &mut self,
    ) -> Result<Vec<AdapterEgress>, AdapterRuntimeError> {
        let id = self.next_id()?;
        let request = self.inner.decline_native_turn(id).map_err(map_adapter)?;
        let Ok(frame) = OutboundFrame::new(request.bytes) else {
            self.inner.reject_before_write(id).map_err(map_adapter)?;
            return Err(AdapterRuntimeError::Frame);
        };
        Ok(vec![AdapterEgress::new(
            frame,
            EgressToken::native_rpc_continuation(id),
        )])
    }

    /// The announced Turn runs under the access Sage last gave the thread.
    pub(super) fn native_turn_authority(&self) -> Option<HarnessTurnAuthority> {
        self.inner
            .native_turn_access()
            .map(HarnessTurnAuthority::Access)
    }
}
