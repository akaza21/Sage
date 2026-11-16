use super::*;

impl ClaudeRuntime {
    /// Opens the Turn the next prompt request answers, in `native_mode`. It
    /// opens only while the session is, by every source it reports, in that
    /// mode, so a flip after the mode was selected stops it here.
    pub(super) fn begin_turn(
        &mut self,
        client_message_id: RequiredText<512>,
        native_mode: &'static str,
    ) -> Result<u64, AdapterRuntimeError> {
        let id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        let external_turn_id = ExternalId::new("externalTurnId", format!("prompt-{id}"))
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() || !active.config.in_mode(native_mode) {
            return Err(AdapterRuntimeError::Operation);
        }
        active
            .translator
            .begin_prompt(&id.to_string())
            .map_err(map_translation)?;
        active.turn = Some(TurnIntent {
            external_turn_id,
            client_message_id,
            request_id: id,
            cancel_written: false,
            native_mode: Some(native_mode),
            sanctioned_mode: None,
        });
        self.selected_mode = Some(native_mode);
        Ok(id)
    }

    /// Whether a Turn Claude Code started on its own may open: the session
    /// is in the mode Sage last selected for it, no setting is changing, and
    /// a request slot is free for the Turn's end.
    pub(crate) fn harness_turn_admitted(&self) -> bool {
        self.delivery_unknown.is_none()
            && self.pending.len() < MAX_PENDING_REQUESTS
            && !self.config_pending()
            && self.active.as_ref().is_some_and(|active| {
                active.turn.is_none()
                    && self
                        .selected_mode
                        .is_some_and(|mode| active.config.in_mode(mode))
            })
    }

    /// Opens the Turn Claude Code started on its own, as the next prompt
    /// request would, in the mode Sage last selected. Returns the request id
    /// its end answers.
    pub(crate) fn begin_harness_turn(
        &mut self,
        client_message_id: RequiredText<512>,
    ) -> Result<u64, AdapterRuntimeError> {
        if !self.harness_turn_admitted() {
            return Err(AdapterRuntimeError::Operation);
        }
        let mode = self.selected_mode.ok_or(AdapterRuntimeError::Operation)?;
        let id = self.begin_turn(client_message_id, mode)?;
        self.pending.insert(id, PendingRequest::Prompt);
        self.next_request_id = id;
        Ok(id)
    }

    /// The mode Sage last selected for the session.
    pub(crate) fn selected_mode(&self) -> Option<&'static str> {
        self.selected_mode
    }

    /// The session's own identity, for a Turn it starts before Sage names it.
    pub(crate) fn harness_session_identity(
        &self,
    ) -> Result<crate::acp::bindings::adapter::AdapterIdentity, AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        Ok(self.session_identity(active.external_id.clone()))
    }
}
