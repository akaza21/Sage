//! Undoing the latest Turn: Codex drops it, and every later one, from the
//! thread's history. Files the Turn changed stay as they are.

use super::{AdapterError, CodexAdapter, RequestKind};
use serde_json::Value;

use crate::acp::bindings::codex::request::OutgoingRequest;
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::contract::SafeText;

impl CodexAdapter {
    pub(crate) fn rewind_turns_page(
        &mut self,
        id: u64,
        cursor: Option<&str>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|m| {
            m.snapshot.rewind.effects.conversation || m.snapshot.rewind.fork_at
        })?;
        if self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
            || self.pending_read.is_some()
            || self.pending_fork.is_some()
            || self.revert_pending()
        {
            return Err(AdapterError::TurnAlreadyActive);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        self.reserve(OutgoingRequest::turns_list(id, &thread, cursor, true)?)
    }
    pub(crate) fn native_target(
        &self,
        target: &sage_acp_protocol::session_control::ConversationTarget,
    ) -> Result<String, AdapterError> {
        target
            .native_turn_id
            .as_ref()
            .map(|id| id.as_str().to_owned())
            .or_else(|| {
                self.previous_turn
                    .as_ref()
                    .filter(|turn| turn.client_message_id.as_str() == target.turn_id.as_str())
                    .map(|turn| turn.id.as_str().to_owned())
            })
            .filter(|id| !id.starts_with(super::REJECTED_TURN))
            .ok_or(AdapterError::ForeignRequest)
    }

    pub(crate) fn revert_to(
        &mut self,
        id: u64,
        target: &sage_acp_protocol::session_control::ConversationTarget,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|m| m.snapshot.rewind.effects.conversation)?;
        let turn = self.native_target(target)?;
        if self.revert_pending()
            || self.pending_fork.is_some()
            || self.turn_start_correlations.has_pending()
            || self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
        {
            return Err(AdapterError::TurnAlreadyActive);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = self.reserve(OutgoingRequest::thread_revert(id, &thread, &turn)?)?;
        self.revert_target = Some(turn);
        Ok(request)
    }

    pub(crate) fn reverted_target(
        &self,
        target: &sage_acp_protocol::session_control::ConversationTarget,
    ) -> bool {
        self.native_target(target).ok().as_deref() == self.reverted_turn.as_deref()
            && self.reverted_turn.is_some()
    }

    pub(crate) fn cut_fork(
        &mut self,
        id: u64,
        fork: &sage_acp_protocol::intent::ExternalId,
        turn: &str,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.reserve(OutgoingRequest::thread_revert(id, fork, turn)?)
    }

    pub(crate) fn complete_fork_cut(&mut self, id: u64) -> Result<(), AdapterError> {
        if self.take_pending(id) != Some(RequestKind::ThreadRevert) {
            return Err(AdapterError::ForeignRequest);
        }
        Ok(())
    }
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
            .filter(|turn| !turn.id.as_str().starts_with(super::REJECTED_TURN))
            .map(|turn| turn.id.as_str().to_owned())
            .ok_or(AdapterError::ForeignRequest)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = self.reserve(OutgoingRequest::thread_revert(id, &thread, &turn)?)?;
        self.revert_target = Some(turn);
        Ok(request)
    }

    pub(crate) fn reverted(&self, client_message_id: &str) -> bool {
        self.previous_turn.as_ref().is_some_and(|turn| {
            turn.client_message_id.as_str() == client_message_id
                && self.reverted_turn.as_deref() == Some(turn.id.as_str())
        })
    }

    pub(crate) fn revert_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|(_, kind)| *kind == RequestKind::ThreadRevert)
    }

    pub(crate) fn complete_revert(&mut self, id: u64, error: Option<&Value>) -> NativeRpcReply {
        let removed = self.take_pending(id);
        debug_assert_eq!(removed, Some(RequestKind::ThreadRevert));
        let target = self.revert_target.take();
        let Some(error) = error else {
            self.reverted_turn = target;
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
