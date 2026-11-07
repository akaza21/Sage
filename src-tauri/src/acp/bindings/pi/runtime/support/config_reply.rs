use super::{AdapterEgress, AdapterEvent, AdapterRuntimeError, PiRuntime, identity::model_key};
use crate::acp::bindings::pi::{ModelSummary, StateSnapshot};

type ConfigReply = (Option<AdapterEvent>, Option<AdapterEgress>);

impl PiRuntime {
    pub(super) fn state_reply(
        &mut self,
        state: StateSnapshot,
    ) -> Result<ConfigReply, AdapterRuntimeError> {
        self.current_model = state.model.as_ref().map(model_key).transpose()?;
        self.current_thinking = state.thinking_level;
        let _ = (state.has_session, state.is_streaming);
        if (self.refresh_thinking_after_model || self.available_thinking.is_empty())
            && self.active_turn.is_some()
            && self.thinking_request_command.is_none()
        {
            let levels = self.begin_thinking_discovery()?;
            Ok((Some(AdapterEvent::IgnoredDuplicate), Some(levels)))
        } else {
            Ok((
                self.initialized.then_some(AdapterEvent::IgnoredDuplicate),
                None,
            ))
        }
    }

    pub(super) fn model_selected(
        &mut self,
        model: ModelSummary,
    ) -> Result<ConfigReply, AdapterRuntimeError> {
        let selected = model_key(&model)?;
        if !self
            .available_models
            .iter()
            .any(|candidate| model_key(candidate).ok().as_deref() == Some(selected.as_str()))
        {
            return Err(AdapterRuntimeError::Frame);
        }
        self.current_model = Some(selected);
        self.current_thinking = None;
        self.available_thinking.clear();
        self.refresh_thinking_after_model = true;
        Ok((Some(AdapterEvent::IgnoredDuplicate), None))
    }

    pub(super) fn thinking_levels(&mut self, levels: Vec<String>) -> ConfigReply {
        if self.thinking_request_session.as_ref() != self.external_session.as_ref()
            || self.thinking_request_owner.as_ref()
                != Some(&(self.session_id.clone(), self.runtime_epoch))
        {
            self.clear_thinking_request();
            return (Some(AdapterEvent::IgnoredLate), None);
        }
        self.clear_thinking_request();
        self.available_thinking = levels;
        self.refresh_thinking_after_model = false;
        (Some(AdapterEvent::IgnoredDuplicate), None)
    }

    pub(super) fn thinking_selected(
        &mut self,
        level: String,
    ) -> Result<ConfigReply, AdapterRuntimeError> {
        if !self
            .available_thinking
            .iter()
            .any(|available| available == &level)
        {
            return Err(AdapterRuntimeError::Frame);
        }
        self.current_thinking = Some(level);
        Ok((Some(AdapterEvent::IgnoredDuplicate), None))
    }
}

impl PiRuntime {
    pub(super) fn command_session_reset(&mut self) -> Result<ConfigReply, AdapterRuntimeError> {
        self.commands = std::sync::Arc::default();
        self.commands_pending = false;
        self.commands_discovered = false;
        self.next_session = self
            .next_session
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        let external = crate::acp::intent::ExternalId::new(
            "piSessionId",
            format!(
                "pi-session-{}-{}",
                self.runtime_epoch.get(),
                self.next_session
            ),
        )
        .map_err(|_| AdapterRuntimeError::Operation)?;
        self.external_session = Some(external.clone());
        self.recent_cancelled_dialogs.clear();
        self.available_thinking.clear();
        Ok((
            Some(AdapterEvent::SessionReady {
                id: self.identity(external, None, None),
                loaded: false,
            }),
            None,
        ))
    }
}
