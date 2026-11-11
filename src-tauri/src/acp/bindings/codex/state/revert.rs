//! Undoing the latest Turn: Codex drops it, and every later one, from the
//! thread's history. Files the Turn changed stay as they are.

use super::{AdapterError, CodexAdapter, RequestKind};
use serde_json::Value;

use crate::acp::bindings::codex::request::OutgoingRequest;
use crate::acp::{contract::SafeText, goal::NativeRpcReply};

impl CodexAdapter {
    /// `thread/revert` for the Turn Sage sent as `client_message_id`, which
    /// must be the last Turn this runtime ran and finished.
    pub(crate) fn revert(
        &mut self,
        id: u64,
        client_message_id: &str,
    ) -> Result<OutgoingRequest, AdapterError> {
        if self.revert_pending()
            || self.pending_fork.is_some()
            || self.turn_start_correlations.has_pending()
            || self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
        {
            return Err(AdapterError::TurnAlreadyActive);
        }
        let turn = self
            .previous_turn
            .as_ref()
            .filter(|turn| turn.client_message_id.as_str() == client_message_id)
            .filter(|turn| self.reverted_turn.as_deref() != Some(turn.id.as_str()))
            .map(|turn| turn.id.as_str().to_owned())
            .ok_or(AdapterError::ForeignRequest)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        self.reserve(OutgoingRequest::thread_revert(id, &thread, &turn)?)
    }

    pub(crate) fn revert_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|(_, kind)| *kind == RequestKind::ThreadRevert)
    }

    /// Codex answered the revert: once it succeeded the Turn is gone, and
    /// undoing it again is refused. A refusal carries Codex's reason.
    pub(crate) fn complete_revert(&mut self, id: u64, error: Option<&Value>) -> NativeRpcReply {
        let removed = self.take_pending(id);
        debug_assert_eq!(removed, Some(RequestKind::ThreadRevert));
        let Some(error) = error else {
            self.reverted_turn = self
                .previous_turn
                .as_ref()
                .map(|turn| turn.id.as_str().to_owned());
            return NativeRpcReply::Controlled(None);
        };
        let mut reason = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex could not undo the turn.")
            .to_owned();
        let mut end = reason.len().min(512);
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        reason.truncate(end);
        NativeRpcReply::ControlRefused(SafeText::new("controlSummary", reason).ok())
    }
}
