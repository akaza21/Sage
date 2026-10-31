use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterIdentity, AdapterModel, AdapterRuntimeError},
    contract::RequiredText,
    intent::{ExternalId, ModelId},
    limits::LIMITS,
};

use super::super::OmpRuntime;
use crate::acp::bindings::omp::ModelSummary;

impl OmpRuntime {
    pub(super) fn identity(
        &self,
        external_id: ExternalId,
        turn_id: Option<ExternalId>,
        client: Option<RequiredText<512>>,
    ) -> AdapterIdentity {
        AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id,
            turn_id,
            turn_client_message_id: client,
        }
    }

    pub(super) fn turn_identity(&self) -> Result<AdapterIdentity, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(self.identity(
            turn.external_id.clone(),
            Some(turn.external_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn item_identity(
        &self,
        external_id: ExternalId,
    ) -> Result<AdapterIdentity, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?;
        Ok(self.identity(
            external_id,
            Some(turn.external_id.clone()),
            Some(turn.client_message_id.clone()),
        ))
    }

    pub(super) fn require_turn(&self, turn_key: &str) -> Result<(), AdapterRuntimeError> {
        match self.active_turn.as_ref() {
            Some(turn) if turn.external_id.as_str() == turn_key => Ok(()),
            _ => Err(AdapterRuntimeError::Operation),
        }
    }

    pub(in crate::acp::bindings::omp::runtime) fn validate_model(
        &self,
        model: Option<&ModelId>,
    ) -> Result<(), AdapterRuntimeError> {
        match model {
            None => Ok(()),
            Some(model) if self.current_model.as_deref() == Some(model.as_str()) => Ok(()),
            Some(_) => Err(AdapterRuntimeError::UnsupportedCommand),
        }
    }

    pub(super) fn models_event(
        &mut self,
        models: Vec<ModelSummary>,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        if models.len() > LIMITS.wire.models_or_config_entries as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let bytes = models.iter().try_fold(0_usize, |total, model| {
            total
                .checked_add(model.external_id.as_str().len())?
                .checked_add(model.name.len())
        });
        if bytes.is_none_or(|bytes| bytes > LIMITS.wire.models_or_config_bytes as usize) {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        if models.iter().filter(|model| model.is_current).count() > 1 {
            return Err(AdapterRuntimeError::Frame);
        }
        self.current_model = models
            .iter()
            .find(|model| model.is_current)
            .map(|model| model.external_id.as_str().to_owned());
        self.model_count = models
            .len()
            .try_into()
            .map_err(|_| AdapterRuntimeError::BatchCount)?;
        let models = models
            .into_iter()
            .map(|model| {
                Ok(AdapterModel {
                    context_limits: Default::default(),
                    id: model.external_id,
                    name: RequiredText::new("modelName", model.name)
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                    is_default: model.is_current,
                })
            })
            .collect::<Result<Vec<_>, AdapterRuntimeError>>()?;
        Ok(AdapterEvent::Models {
            models,
            next_cursor: None,
        })
    }
}
