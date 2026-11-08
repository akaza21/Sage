//! Keeps a running turn inside the permission mode Sage selected for it.
//! Claude can change its own mode mid-turn (entering plan mode, or a
//! plan-exit option that switches to auto or bypass). Moving to a mode no
//! more permissive is fine, and so is the mode the user's own answer to a
//! permission request leads to. Anything else is a policy violation: Sage
//! cancels the turn and says why in the chat.

use super::*;
use crate::acp::bindings::claude::{mode_change_admitted, option_target_mode};
use crate::acp::intent::session_event::{SessionNotice, SessionNoticeLevel, SessionScopedEvent};

impl ClaudeRuntime {
    /// Remembers the mode a chosen permission option moves the turn into.
    pub(super) fn sanction_option(&mut self, option_id: &str) {
        if let Some(turn) = self.active.as_mut().and_then(|active| active.turn.as_mut()) {
            turn.sanctioned_mode = option_target_mode(option_id).or(turn.sanctioned_mode);
        }
    }

    /// Whether the session moving to `next` stays within what the running
    /// turn was given. Outside a turn the next prompt selects its own mode.
    pub(super) fn mode_change_allowed(&self, next: &str) -> bool {
        let Some(turn) = self.active.as_ref().and_then(|active| active.turn.as_ref()) else {
            return true;
        };
        // A turn without a selected mode runs as Claude's default.
        let selected = turn.native_mode.unwrap_or("default");
        mode_change_admitted(selected, next) || turn.sanctioned_mode == Some(next)
    }

    /// Cancels the running turn and tells the chat why.
    pub(super) fn mode_violation(&self, next: &str) -> Result<AdapterBatch, AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        let selected = active
            .turn
            .as_ref()
            .and_then(|turn| turn.native_mode)
            .unwrap_or("default");
        tracing::warn!(
            session_id = self.session_id.as_str(),
            selected_mode = selected,
            reported_mode = next,
            "Claude left the turn's permission mode; cancelling the turn"
        );
        let text = format!(
            "Claude switched this chat to its \"{next}\" permission mode, which the chat's access \
             doesn't allow. Sage stopped the turn. Your next message runs in the chat's own access mode."
        );
        let notice = SessionNotice {
            level: SessionNoticeLevel::Error,
            text: RequiredText::new("notice", text).map_err(|_| AdapterRuntimeError::Frame)?,
            source: None,
        };
        let event = SharedEvent::Session {
            id: self.session_identity(active.external_id.clone()),
            event: SessionScopedEvent::Notice(notice),
        };
        let cancel = cancel_notification(
            "session/cancel",
            json!({"sessionId": active.external_id.as_str()}),
        )?;
        AdapterBatch::new(vec![event], vec![cancel])
    }
}
