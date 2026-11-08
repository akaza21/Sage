//! The model catalog an adapter reports while a session activates.

use super::*;

impl<A: HarnessAdapter, J: ActivationJournal, R: ResourceMapper> ActivationPump<A, J, R> {
    pub(super) fn accept_models(
        &mut self,
        values: Vec<AdapterModel>,
        next_cursor: Option<PageCursor>,
    ) -> Result<(), ActivationPumpError> {
        if !matches!(self.phase, Phase::Initialized | Phase::AwaitingSession) {
            return self.protocol_error();
        }
        let count = self
            .models
            .len()
            .checked_add(values.len())
            .ok_or(ActivationPumpError::Capacity)?;
        if count > LIMITS.wire.models_or_config_entries as usize {
            return self.capacity_error();
        }
        let mut candidate = Vec::new();
        candidate
            .try_reserve_exact(count)
            .map_err(|_| ActivationPumpError::Capacity)?;
        candidate.extend(self.models.iter().cloned());
        candidate.extend(values);
        if crate::acp::semantic_ingress::validate_models(&candidate).is_err() {
            return self.capacity_error();
        }
        self.models = candidate;
        self.next_model_cursor = next_cursor;
        Ok(())
    }
}
