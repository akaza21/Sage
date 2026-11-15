use super::*;

impl ClaudeRuntime {
    /// Opens the Turn Claude Code started on its own, as the next prompt
    /// request would. Returns the request id its end answers.
    pub(crate) fn begin_harness_turn(
        &mut self,
        client_message_id: RequiredText<512>,
    ) -> Result<u64, AdapterRuntimeError> {
        if self.delivery_unknown.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        if self.pending.len() >= MAX_PENDING_REQUESTS {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let external_turn_id = ExternalId::new("externalTurnId", format!("prompt-{id}"))
            .map_err(|_| AdapterRuntimeError::Operation)?;
        active
            .translator
            .begin_prompt(&id.to_string())
            .map_err(map_translation)?;
        active.turn = Some(TurnIntent {
            external_turn_id,
            client_message_id,
            request_id: id,
            cancel_written: false,
            // It runs in whatever mode the session is in.
            native_mode: super::super::current_mode(&active.config),
            sanctioned_mode: None,
        });
        self.pending.insert(id, PendingRequest::Prompt);
        self.next_request_id = id;
        Ok(id)
    }

    /// The session's own identity, for a Turn it starts before Sage names it.
    pub(crate) fn harness_session_identity(
        &self,
    ) -> Result<crate::acp::bindings::adapter::AdapterIdentity, AdapterRuntimeError> {
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
        Ok(self.session_identity(active.external_id.clone()))
    }
}
