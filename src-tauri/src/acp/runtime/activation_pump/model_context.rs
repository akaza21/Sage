use super::*;
use crate::acp::context_handoff::budget::ModelContextLimits;

impl<A: HarnessAdapter, J: ActivationJournal, R: ResourceMapper> ActivationPump<A, J, R> {
    pub(super) fn activation_model_context(&self, ready: &ActivatedSession) -> ModelContextLimits {
        let confirmed = self
            .adapter
            .session_config()
            .and_then(|config| config.confirmed_model);
        let selected = confirmed
            .as_ref()
            .map(|model| &model.id)
            .or(match &self.intent.mode {
                ActivationMode::New { model, .. } | ActivationMode::Resume { model, .. } => {
                    model.as_ref()
                }
                ActivationMode::Load { .. } => None,
            });
        ready
            .models
            .iter()
            .find(|model| selected.map_or(model.is_default, |id| &model.id == id))
            .map_or_else(ModelContextLimits::default, |model| model.context_limits)
    }
}
