use crate::{
    acp::{
        bindings::codex::{request::OutgoingRequest, turn_policy::TurnPolicySurface},
        contract::RequiredText,
        intent::Prompt,
    },
    settings_store::SessionAccessMode,
};

use super::{AdapterError, CodexAdapter};

impl CodexAdapter {
    pub(crate) fn steer(
        &mut self,
        id: u64,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.active_turn_steer)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let turn = self
            .active_turn_id
            .clone()
            .ok_or(AdapterError::TurnUnavailable)?;
        self.reserve(OutgoingRequest::steer(
            id,
            &thread,
            &turn,
            prompt,
            client_message_id,
        )?)
    }

    pub(crate) fn prompt(
        &mut self,
        id: u64,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.prompt_with_access_mode(id, prompt, client_message_id, SessionAccessMode::ReadOnly)
    }

    pub(crate) fn prompt_with_access_mode(
        &mut self,
        id: u64,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
        access_mode: SessionAccessMode,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.prompt_with_effort(id, prompt, client_message_id, access_mode, None)
    }

    pub(crate) fn prompt_with_effort(
        &mut self,
        id: u64,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
        access_mode: SessionAccessMode,
        effort: Option<&RequiredText<512>>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.prompt)?;
        if self.turn_start_correlations.has_pending()
            || self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
        {
            return Err(AdapterError::TurnAlreadyActive);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = match &prompt.action {
            Some(action) => {
                self.require_method(|methods| action.admitted_by(&methods.snapshot))?;
                OutgoingRequest::session_action(id, &thread, action)?
            }
            None => OutgoingRequest::turn_start_with_access_mode(
                id,
                &thread,
                prompt,
                client_message_id,
                access_mode,
                TurnPolicySurface::from_identity(
                    self.observed_build_characterisation,
                    &self.runtime_version,
                ),
                effort,
            )?,
        };
        // A listed skill runs attached; its typed text follows it.
        let request = match self.skill_input(prompt) {
            Some((name, path)) if prompt.action.is_none() => request.with_skill(&name, &path)?,
            _ => request,
        };
        let request = self.reserve(request)?;
        self.turn_start_correlations
            .register(id, client_message_id.clone());
        Ok(request)
    }
}
