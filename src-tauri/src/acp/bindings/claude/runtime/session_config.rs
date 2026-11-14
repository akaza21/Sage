use super::*;
use crate::acp::session_config::{ConfigId, ConfigIntent, ConfigMutation, SessionConfig};

impl ClaudeRuntime {
    pub(super) fn configure_session(
        &mut self,
        intent: &ConfigIntent,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if intent.owner.session_id != self.session_id
            || intent.owner.runtime_epoch != self.runtime_epoch
            || self.config_pending()
        {
            return Err(AdapterRuntimeError::Operation);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        // A turn runs in the mode selected before its prompt; nothing moves
        // it while it runs.
        if active.turn.is_some() && active.config.changes_mode(&intent.mutation) {
            return Err(AdapterRuntimeError::Operation);
        }
        let session_id = active.external_id.clone();
        match &intent.mutation {
            ConfigMutation::Mode(mode) => {
                active
                    .config
                    .validate_mode(mode.as_str())
                    .map_err(|_| AdapterRuntimeError::Operation)?;
                self.reserve(
                    PendingRequest::SetMode,
                    "session/set_mode",
                    json!({"sessionId":session_id,"modeId":mode}),
                )
            }
            ConfigMutation::Option { config_id, value } => {
                active
                    .config
                    .validate_value(config_id.as_str(), value)
                    .map_err(|_| AdapterRuntimeError::Operation)?;
                let mut params = value.wire();
                params["sessionId"] = json!(session_id);
                params["configId"] = json!(config_id);
                self.reserve(
                    PendingRequest::SetConfig,
                    "session/set_config_option",
                    params,
                )
            }
        }
    }
    pub(super) fn config_response(
        &mut self,
        value: Value,
        replace: bool,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if let Some(error) = value.get("error") {
            let valid = error.as_object().is_some_and(|error| {
                error.get("code").and_then(Value::as_i64).is_some()
                    && error.get("message").and_then(Value::as_str).is_some()
            });
            if !valid || value.get("result").is_some() {
                return Err(AdapterRuntimeError::Frame);
            }
            return Err(AdapterRuntimeError::ConfigMutationRejected);
        }
        let result = value
            .get("result")
            .filter(|v| v.is_object())
            .ok_or(AdapterRuntimeError::Frame)?;
        if replace {
            let options = result
                .get("configOptions")
                .ok_or(AdapterRuntimeError::Frame)?;
            let before = self.reported_mode();
            self.replace_config_options(options)?;
            if let Some(next) = self
                .reported_mode()
                .filter(|next| before.as_ref() != Some(next))
                && !self.mode_change_allowed(&next)
            {
                return self.mode_violation(&next);
            }
        }
        // set_mode's metadata-only acknowledgement must never echo the request.
        AdapterBatch::new(vec![SharedEvent::IgnoredDuplicate], Vec::new())
    }
    fn replace_config_options(&mut self, options: &Value) -> Result<(), AdapterRuntimeError> {
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let candidate = SessionConfig::from_wire(
            &json!({"modes":active.config.modes,"configOptions":options}),
            true,
        )
        .map_err(|_| AdapterRuntimeError::Frame)?;
        active.config = candidate;
        // Options reported alone still move the mode they restate.
        active.config.follow_mode_option();
        Ok(())
    }
    fn reported_mode(&self) -> Option<String> {
        self.active
            .as_ref()
            .and_then(|active| active.config.current_mode())
            .map(str::to_owned)
    }
    pub(super) fn config_notification(
        &mut self,
        value: &Value,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        let update = &value["params"]["update"];
        let kind = update["sessionUpdate"].as_str().unwrap_or_default();
        if !matches!(kind, "current_mode_update" | "config_option_update") {
            return Ok(None);
        }
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        if value["params"]["sessionId"].as_str() != Some(active.external_id.as_str()) {
            return Err(AdapterRuntimeError::Frame);
        }
        let before = active.config.current_mode().map(str::to_owned);
        // The mode Claude now reports: the restated mode itself, or the
        // `mode` option's value when an option update moved it.
        let reported = match kind {
            "current_mode_update" => {
                let id: ConfigId = serde_json::from_value(update["currentModeId"].clone())
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                active
                    .config
                    .set_current_mode(id.clone())
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                Some(id.as_str().to_owned())
            }
            _ => {
                self.replace_config_options(
                    update
                        .get("configOptions")
                        .ok_or(AdapterRuntimeError::Frame)?,
                )?;
                let active = self.active.as_ref().ok_or(AdapterRuntimeError::Frame)?;
                active
                    .config
                    .current_mode()
                    .filter(|mode| before.as_deref() != Some(*mode))
                    .map(str::to_owned)
            }
        };
        if let Some(next) = reported
            && !self.mode_change_allowed(&next)
        {
            return self.mode_violation(&next).map(Some);
        }
        AdapterBatch::new(vec![SharedEvent::IgnoredDuplicate], Vec::new()).map(Some)
    }
}

pub(super) fn client_capabilities() -> Value {
    let mut capabilities = crate::acp::runtime::host_browse::advertised_host_client_capabilities();
    capabilities["session"] = json!({"configOptions":{"boolean":{}}});
    capabilities
}
