use super::{CodexRuntime, support::config_for_model};
use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
    intent::{ModelId, PageCursor},
    limits::LIMITS,
    session_config::{ConfigIntent, ConfigMutation, ConfigValue},
};

impl CodexRuntime {
    pub(super) fn discard_invalid_model_page(
        &mut self,
        frame: &crate::acp::transport::Frame,
    ) -> Result<crate::acp::bindings::adapter::AdapterBatch, AdapterRuntimeError> {
        let id = crate::acp::bindings::adapter::response_id_for_discard(frame)
            .ok_or(AdapterRuntimeError::Frame)?;
        if self.inner.pending_kind(id) != Some(super::super::request::RequestKind::Models) {
            return Err(AdapterRuntimeError::Frame);
        }
        let removed = self.inner.take_pending(id);
        debug_assert_eq!(removed, Some(super::super::request::RequestKind::Models));
        self.next_model_cursor = None;
        super::support::one(AdapterEvent::IgnoredLate)
    }

    pub(super) fn reset_session_model(&mut self, model: Option<ModelId>, resume: bool) {
        self.selected_model_id = model;
        self.selected_model = None;
        self.session_config = None;
        self.selected_effort = None;
        self.model_list_requested = false;
        self.model_catalog_idle_only = false;
        self.model_catalog_entries = 0;
        self.model_catalog_bytes = 0;
        self.next_model_cursor = None;
        self.model_pages_seen = 0;
        self.session_is_resume = resume;
    }

    pub(super) fn refresh_model_config(&mut self) {
        let Some(model) = self.selected_model.as_ref() else {
            return;
        };
        let effort = self.selected_effort.as_ref().or_else(|| {
            (!self.session_is_resume)
                .then_some(model.default_effort.as_ref())
                .flatten()
        });
        self.session_config = effort.and_then(|effort| config_for_model(model, effort.as_str()));
    }

    pub(super) fn apply_effort(&mut self, effort: crate::acp::contract::RequiredText<512>) {
        self.selected_effort = Some(effort);
        self.refresh_model_config();
    }

    pub(super) fn configure_effort(
        &mut self,
        intent: &ConfigIntent,
    ) -> Result<(), AdapterRuntimeError> {
        if intent.owner.session_id != *self.inner.session_id()
            || intent.owner.runtime_epoch != self.inner.runtime_epoch()
            || self.inner.has_active_turn()
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let ConfigMutation::Option {
            config_id,
            value: config_value @ ConfigValue::Select { value: selected },
        } = &intent.mutation
        else {
            return Err(AdapterRuntimeError::Operation);
        };
        if config_id.as_str() != "effort" {
            return Err(AdapterRuntimeError::Operation);
        }
        self.session_config
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .validate_value(config_id.as_str(), config_value)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let effort = self
            .selected_model
            .as_ref()
            .and_then(|model| {
                model
                    .supported_efforts
                    .iter()
                    .find(|effort| effort.id.as_str() == selected.as_str())
            })
            .ok_or(AdapterRuntimeError::Operation)?
            .id
            .clone();
        self.apply_effort(effort);
        Ok(())
    }

    pub(super) fn request_model_config(&mut self) -> Option<AdapterEgress> {
        if self.model_list_requested {
            return self.request_next_model_page();
        }
        let egress = self.request(|adapter, id| adapter.models(id, None)).ok()?;
        self.model_list_requested = true;
        Some(egress)
    }

    fn request_next_model_page(&mut self) -> Option<AdapterEgress> {
        if self.model_pages_seen >= LIMITS.codex.model_catalog_pages {
            return None;
        }
        let cursor = self.next_model_cursor.take()?;
        self.request(|adapter, id| adapter.models(id, Some(&cursor)))
            .ok()
    }

    fn reject_model_page(&mut self) -> (AdapterEvent, Option<AdapterEgress>) {
        self.next_model_cursor = None;
        (AdapterEvent::IgnoredLate, None)
    }

    pub(super) fn apply_model_page(
        &mut self,
        models: Vec<super::super::incoming::ModelDescriptor>,
        next_cursor: Option<PageCursor>,
    ) -> (AdapterEvent, Option<AdapterEgress>) {
        let selected = models
            .iter()
            .find(|model| {
                self.selected_model_id
                    .as_ref()
                    .map_or(model.is_default, |id| id.as_str() == model.id.as_str())
            })
            .cloned();
        let Ok(models) = super::support::model_values(models) else {
            return self.reject_model_page();
        };
        let entries = self.model_catalog_entries.saturating_add(models.len());
        let Some(bytes) = models
            .iter()
            .try_fold(self.model_catalog_bytes, |total, model| {
                total.checked_add(model.actual_bytes()?)
            })
        else {
            return self.reject_model_page();
        };
        if entries > LIMITS.wire.models_or_config_entries as usize
            || bytes > LIMITS.wire.models_or_config_bytes as usize
        {
            return self.reject_model_page();
        }
        self.model_catalog_entries = entries;
        self.model_catalog_bytes = bytes;
        self.next_model_cursor = next_cursor.clone();
        self.model_pages_seen += 1;
        if let Some(model) = selected {
            self.selected_model = Some(model);
            self.refresh_model_config();
        }
        if self.model_catalog_idle_only {
            (AdapterEvent::IgnoredLate, None)
        } else {
            let egress = if self.inner.has_bound_thread() {
                self.request_next_model_page()
            } else {
                None
            };
            (
                AdapterEvent::Models {
                    models,
                    next_cursor,
                },
                egress,
            )
        }
    }
}
