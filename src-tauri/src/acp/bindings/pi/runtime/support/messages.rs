use super::{AdapterEgress, AdapterEvent, AdapterRuntimeError, PiRuntime, identity};
use crate::acp::bindings::pi::{ModelSummary, Operation, StateSnapshot};
use sage_acp_protocol::{intent::NormalizedPayload, lifecycle::ItemLifecycle};

impl PiRuntime {
    pub(super) fn message_item(
        &mut self,
        payload: NormalizedPayload,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = self.next_local_item("message")?;
        self.item(id, ItemLifecycle::Completed, None, payload)
    }

    pub(super) fn configuration_reply(
        &mut self,
        state: StateSnapshot,
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        let model = state.model.as_ref().map(identity::model_key).transpose()?;
        self.model_check_due = false;
        if model == self.current_model && !self.configuration_dirty {
            // Unchanged: the prompt's ordinary state read.
            return self.state_reply(state);
        }
        self.refresh_thinking_after_model = true;
        self.current_model = model;
        self.current_thinking = state.thinking_level;
        self.auto_compaction = state.auto_compaction;
        // Optional like the read that asked for it; it stays due if skipped.
        let models = self
            .begin_one(Operation::RefreshModels, super::super::RequestKind::Models)
            .ok();
        self.configuration_models_pending = models.is_some();
        Ok((Some(AdapterEvent::IgnoredDuplicate), models))
    }

    pub(super) fn models_reply(
        &mut self,
        models: Vec<ModelSummary>,
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        let event = identity::model_event(models.clone(), self.current_model.as_deref())?;
        if let AdapterEvent::Models { models, .. } = &event {
            self.model_count = models
                .len()
                .try_into()
                .map_err(|_| AdapterRuntimeError::BatchCount)?;
        }
        self.available_models = models;
        // A configuration read is done once its models arrive, and continues
        // to the thinking levels.
        let configuration = std::mem::take(&mut self.configuration_models_pending);
        if configuration {
            self.configuration_dirty = false;
        }
        let levels = if configuration
            && self.initialized
            && self.active_turn.is_some()
            && self.refresh_thinking_after_model
            && self.thinking_request_command.is_none()
        {
            Some(self.begin_thinking_discovery()?)
        } else {
            None
        };
        Ok((Some(event), levels))
    }

    /// An extension may switch the model without any event; the model that
    /// answered the Turn tells.
    pub(super) fn note_reply_model(&mut self) {
        if self
            .translator
            .reply_model()
            .is_some_and(|model| Some(model) != self.current_model.as_deref())
        {
            self.model_check_due = true;
        }
    }
}
