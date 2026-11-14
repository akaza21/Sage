use super::super::PiRuntime;
use super::identity::model_key;
use crate::acp::bindings::{
    adapter::{AdapterEgress, AdapterRuntimeError, HarnessAdapter},
    pi::Operation,
};
use sage_acp_protocol::session_config::{ConfigIntent, ConfigMutation, ConfigValue};

const MODEL: &str = "pi.model";
const THINKING: &str = "pi.thinking";
const AUTO_COMPACTION: &str = "pi.autoCompaction";

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn clear_thinking_request(&mut self) {
        self.thinking_request_session = None;
        self.thinking_request_owner = None;
        self.thinking_request_command = None;
    }

    pub(in crate::acp::bindings::pi::runtime) fn begin_thinking_discovery(
        &mut self,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned
            || !self.initialized
            || self.external_session.is_none()
            || (self.active_turn.is_some()
                && !self.refresh_thinking_after_model
                && !self.available_thinking.is_empty())
            || self.thinking_request_command.is_some()
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let egress = self.begin_one(
            Operation::GetThinkingLevels,
            super::super::RequestKind::ThinkingLevels,
        )?;
        self.thinking_request_session = self.external_session.clone();
        self.thinking_request_owner = Some((self.session_id.clone(), self.runtime_epoch));
        self.thinking_request_command = self
            .translator
            .thinking_levels_request_id()
            .map(str::to_owned);
        Ok(egress)
    }

    pub(in crate::acp::bindings::pi::runtime) fn configure_session(
        &mut self,
        intent: &ConfigIntent,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned
            || !self.initialized
            || self.external_session.is_none()
            || self.active_turn.is_some()
            || self.config_pending()
            || intent.owner.session_id != self.session_id
            || intent.owner.runtime_epoch != self.runtime_epoch
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let ConfigMutation::Option { config_id, value } = &intent.mutation else {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        };
        self.model_session_config()
            .ok_or(AdapterRuntimeError::Operation)?
            .validate_value(config_id.as_str(), value)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        match (config_id.as_str(), value) {
            (MODEL, ConfigValue::Select { value }) => {
                let model = self
                    .available_models
                    .iter()
                    .find(|model| model_key(model).ok().as_deref() == Some(value.as_str()))
                    .ok_or(AdapterRuntimeError::Operation)?;
                self.begin_one(
                    Operation::SetModel {
                        provider: model.provider.clone(),
                        id: model.id.clone(),
                    },
                    super::super::RequestKind::SetModel,
                )
            }
            (THINKING, ConfigValue::Select { value }) => self.begin_one(
                Operation::SetThinkingLevel {
                    level: value.as_str().to_owned(),
                },
                super::super::RequestKind::SetThinkingLevel,
            ),
            (AUTO_COMPACTION, ConfigValue::Boolean { value }) => self.begin_one(
                Operation::SetAutoCompaction { enabled: *value },
                super::super::RequestKind::SetAutoCompaction,
            ),
            _ => Err(AdapterRuntimeError::UnsupportedCommand),
        }
    }

    pub(in crate::acp::bindings::pi::runtime) fn model_session_config(
        &self,
    ) -> Option<sage_acp_protocol::session_config::SessionConfig> {
        if !self.initialized || self.available_models.is_empty() {
            return None;
        }
        let current = self.current_model.as_ref()?;
        let options = self
            .available_models
            .iter()
            .map(|model| {
                let value = model_key(model).ok()?;
                let name = if model.name.len() <= 4096 {
                    &model.name
                } else {
                    &model.id
                };
                Some(serde_json::json!({"value":value,"name":name}))
            })
            .collect::<Option<Vec<_>>>()?;
        let mut config_options = vec![serde_json::json!({
            "id":MODEL,
            "name":"Model",
            "category":"model",
            "type":"select",
            "currentValue":current,
            "options":options
        })];
        if let Some(thinking) = self.current_thinking.as_ref().filter(|thinking| {
            self.available_thinking
                .iter()
                .any(|level| level == *thinking)
        }) {
            let options = self
                .available_thinking
                .iter()
                .map(|level| serde_json::json!({"value":level,"name":level}))
                .collect::<Vec<_>>();
            config_options.push(serde_json::json!({
                "id":THINKING,
                "name":"Thinking",
                "category":"effort",
                "type":"select",
                "currentValue":thinking,
                "options":options
            }));
        }
        // Pi keeps this setting for all its sessions, as its own `/settings` does.
        if let Some(enabled) = self.auto_compaction {
            config_options.push(serde_json::json!({
                "id":AUTO_COMPACTION,
                "name":"Auto-compact",
                "description":"Summarize older context when it is nearly full",
                "type":"boolean",
                "currentValue":enabled
            }));
        }
        sage_acp_protocol::session_config::SessionConfig::from_wire(
            &serde_json::json!({"configOptions":config_options}),
            true,
        )
        .ok()
    }
}
