use super::{
    CodexRuntime,
    support::{effort_option, model_option, thread_config},
};
use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
    intent::{ModelId, PageCursor},
    limits::LIMITS,
    session_config::{ConfigIntent, ConfigMutation, ConfigValue},
};

use super::super::incoming::ModelDescriptor;

/// The models the thread's `model/list` pages named, which its model can be
/// changed to, and the one Sage set for the next `turn/start`. Codex applies
/// a `turn/start` model to that Turn and the ones after it, so the thread's
/// model is a session choice like its effort. Holds at most one listing's
/// bounded entries.
#[derive(Debug, Default)]
pub(super) struct ModelCatalog {
    models: Vec<ModelDescriptor>,
    pub(super) choice: Option<ModelId>,
}

impl ModelCatalog {
    fn extend(&mut self, page: &[ModelDescriptor]) {
        for model in page {
            if self.models.len() >= LIMITS.wire.models_or_config_entries as usize {
                return;
            }
            if !self.models.iter().any(|known| known.id == model.id) {
                self.models.push(model.clone());
            }
        }
    }

    fn find(&self, id: &str) -> Option<&ModelDescriptor> {
        self.models.iter().find(|model| model.id.as_str() == id)
    }
}

impl CodexRuntime {
    pub(super) fn discard_invalid_model_page(
        &mut self,
        frame: &crate::acp::transport::Frame,
    ) -> Result<crate::acp::bindings::adapter::AdapterBatch, AdapterRuntimeError> {
        let id = crate::acp::bindings::adapter::response_id_for_discard(frame)
            .ok_or(AdapterRuntimeError::Frame)?;
        if self.inner.fork_pending() == Some(id) {
            // A fork answer beyond the structural bound is also beyond what
            // `thread/resume` could later load, so no Chat is made for it.
            self.inner.complete_fork(id).map_err(super::map_adapter)?;
            self.native_rpc_reply = Some(crate::acp::goal::NativeRpcReply::TooLarge);
            return super::support::one(AdapterEvent::IgnoredLate);
        }
        if self.inner.pending_kind(id) != Some(super::super::request::RequestKind::Models) {
            return Err(AdapterRuntimeError::Frame);
        }
        let removed = self.inner.take_pending(id);
        debug_assert_eq!(removed, Some(super::super::request::RequestKind::Models));
        self.next_model_cursor = None;
        super::support::one(AdapterEvent::IgnoredLate)
    }

    pub(super) fn reset_session_model(&mut self, model: Option<ModelId>) {
        self.selected_model_id = model;
        self.reported_model_id = None;
        self.selected_model = None;
        self.session_config = None;
        self.selected_effort = None;
        self.model_catalog = ModelCatalog::default();
        self.model_list_requested = false;
        self.model_catalog_idle_only = false;
        self.model_catalog_entries = 0;
        self.model_catalog_bytes = 0;
        self.next_model_cursor = None;
        self.model_pages_seen = 0;
    }

    /// Whether a `model/list` page the thread's options come from is still
    /// unanswered.
    pub(super) fn catalogue_pending(&self) -> bool {
        self.inner
            .awaits(super::super::request::RequestKind::Models)
    }

    /// Adopts the model and effort Codex reports for the thread, on start or
    /// resume and whenever its settings change. The reported model is the
    /// thread's from now on, so a pick not yet sent is not sent over it, and
    /// its listed descriptor offers its efforts at once. An absent effort
    /// keeps the selected one while the model runs it.
    pub(super) fn adopt_reported_settings(
        &mut self,
        model: Option<ModelId>,
        effort: Option<crate::acp::contract::RequiredText<512>>,
    ) {
        if let Some(model) = &model {
            self.model_catalog.choice = None;
            self.selected_model = self.model_catalog.find(model.as_str()).cloned();
            self.selected_model_id = Some(model.clone());
        }
        self.reported_model_id = model;
        let runs = |effort: &crate::acp::contract::RequiredText<512>| {
            self.selected_model.as_ref().is_none_or(|model| {
                model
                    .supported_efforts
                    .iter()
                    .any(|supported| supported.id == *effort)
            })
        };
        self.selected_effort = effort.or(self.selected_effort.take().filter(runs));
        self.refresh_model_config();
    }

    pub(super) fn refresh_model_config(&mut self) {
        let model = self
            .selected_model
            .as_ref()
            .filter(|model| self.reported_model_id.as_ref() == Some(&model.id));
        let effort = self.selected_effort.as_ref();
        // The model is a choice once the catalogue has been read, even when
        // it lists no model the thread runs.
        let catalogue_read =
            self.model_pages_seen > 0 || (self.model_list_requested && !self.catalogue_pending());
        let options = model
            .zip(effort)
            .and_then(|(model, effort)| effort_option(model, effort.as_str()))
            .into_iter()
            .chain(
                self.reported_model_id
                    .as_ref()
                    .filter(|_| catalogue_read)
                    .map(|id| model_option(&self.model_catalog.models, id)),
            );
        let confirmed_model =
            self.reported_model_id
                .as_ref()
                .map(|id| crate::acp::session_config::ConfirmedModel {
                    id: id.clone(),
                    name: model.map(|model| model.name.clone()),
                });
        let known = confirmed_model.is_some() || effort.is_some();
        let mut config = thread_config(
            crate::acp::session_config::SessionConfig {
                confirmed_model,
                confirmed_effort: effort.cloned(),
                ..Default::default()
            },
            options,
        );
        // A model with thinking levels whose current one Codex did not
        // report takes an effort Sage cannot show yet.
        if effort.is_none() && model.is_some_and(|model| !model.supported_efforts.is_empty()) {
            config.unreported = crate::acp::contract::BoundedVec::new(
                "unreported",
                vec![
                    crate::acp::session_config::ConfigId::new("configId", "effort")
                        .expect("fixed option id"),
                ],
            )
            .expect("one option");
        }
        self.session_config =
            (known || !config.config_options.as_slice().is_empty()).then_some(config);
    }

    pub(super) fn apply_effort(&mut self, effort: crate::acp::contract::RequiredText<512>) {
        self.selected_effort = Some(effort);
        self.refresh_model_config();
    }

    /// Changes the thread's model or effort for the next `turn/start`, between
    /// Turns, to a value the session's config offers.
    pub(super) fn configure_thread(
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
        if !matches!(config_id.as_str(), "effort" | "model") {
            return Err(AdapterRuntimeError::Operation);
        }
        self.session_config
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .validate_value(config_id.as_str(), config_value)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        if config_id.as_str() == "model" {
            return self.apply_model(selected.as_str());
        }
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

    /// Runs the thread on the listed model `id` from the next `turn/start`,
    /// at an effort that model runs: the thread's own when the model offers
    /// it, else the model's default, else its first. Codex keeps a thread's
    /// effort until a `turn/start` names another, so one is always named.
    /// Picking the thread's own unlisted model changes nothing.
    fn apply_model(&mut self, id: &str) -> Result<(), AdapterRuntimeError> {
        let Some(model) = self.model_catalog.find(id).cloned() else {
            return if self.reported_model_id.as_ref().map(|model| model.as_str()) == Some(id) {
                Ok(())
            } else {
                Err(AdapterRuntimeError::Operation)
            };
        };
        let offered = |effort: &&crate::acp::contract::RequiredText<512>| {
            model
                .supported_efforts
                .iter()
                .any(|supported| supported.id == **effort)
        };
        self.selected_effort = self
            .selected_effort
            .as_ref()
            .filter(offered)
            .or(model.default_effort.as_ref().filter(offered))
            .or(model.supported_efforts.first().map(|effort| &effort.id))
            .cloned();
        self.model_catalog.choice = Some(model.id.clone());
        self.reported_model_id = Some(model.id.clone());
        self.selected_model_id = Some(model.id.clone());
        self.selected_model = Some(model);
        self.refresh_model_config();
        Ok(())
    }

    /// The `turn/start` for `prompt`, carrying the model and effort chosen
    /// for the thread.
    pub(super) fn prompt_request(
        &mut self,
        prompt: &crate::acp::intent::Prompt,
        client_message_id: &crate::acp::contract::RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let model = self.model_catalog.choice.clone();
        let effort = self.selected_effort.clone();
        let settings = super::super::turn_policy::TurnSettings {
            model: model.as_ref(),
            effort: effort.as_ref(),
        };
        self.request(|adapter, id| {
            adapter.prompt_with_settings(id, prompt, client_message_id, access_mode, settings)
        })
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
        let page = models;
        let Ok(models) = super::support::model_values(&page) else {
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
        self.model_catalog.extend(&page);
        if let Some(model) = self
            .selected_model_id
            .as_ref()
            .and_then(|id| self.model_catalog.find(id.as_str()))
        {
            self.selected_model = Some(model.clone());
        }
        self.refresh_model_config();
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
