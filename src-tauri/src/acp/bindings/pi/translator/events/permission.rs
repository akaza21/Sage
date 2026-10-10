use super::super::{EventPhase, PendingUiRequest, PiTranslator};
use crate::acp::bindings::pi::{
    AdapterError, AdapterOutput, decode::external_id, protocol::is_allow_option,
};
use serde_json::Value;

impl PiTranslator {
    pub(super) fn extension_request(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.authority_denied && turn.terminal.is_none())
        {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            method,
            "notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text"
        ) {
            let live = self
                .active
                .as_ref()
                .is_some_and(|turn| turn.terminal.is_none() && !turn.authority_denied);
            return Ok(vec![if live {
                AdapterOutput::Absorbed
            } else {
                AdapterOutput::LateIgnored
            }]);
        }
        if !matches!(method, "confirm" | "select" | "input" | "editor") {
            if self
                .active
                .as_ref()
                .is_none_or(|turn| turn.terminal.is_some() || turn.authority_denied)
            {
                return Ok(vec![AdapterOutput::LateIgnored]);
            }
            self.live_turn()?;
            return self.unknown_event("extension_ui_request");
        }
        let id = external_id(value.get("id"))?.ok_or(AdapterError::Malformed)?;
        let running = self.active.as_ref().is_some_and(|turn| {
            turn.event_phase == EventPhase::AgentRunning
                && turn.terminal.is_none()
                && !turn.authority_denied
        });
        if !running || matches!(method, "input" | "editor") {
            return Ok(vec![AdapterOutput::DialogCancelled { external_id: id }]);
        }
        let options = if method == "select" {
            let values = value
                .get("options")
                .and_then(Value::as_array)
                .ok_or(AdapterError::Malformed)?;
            if values.len() > 8 {
                return Err(AdapterError::Limit("extension options"));
            }
            let options = values
                .iter()
                .map(|value| {
                    let option = value.as_str().ok_or(AdapterError::Malformed)?;
                    if option.len() > 64 || option.contains('\0') {
                        return Err(AdapterError::Limit("extension option bytes"));
                    }
                    Ok(option.to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            if !options.iter().any(|option| is_allow_option(option)) {
                return Ok(vec![AdapterOutput::DialogCancelled { external_id: id }]);
            }
            options
        } else {
            Vec::new()
        };
        self.remember_pending_ui(PendingUiRequest {
            id: id.clone(),
            method: method.to_owned(),
            options,
        });
        Ok(vec![AdapterOutput::PermissionRequested { external_id: id }])
    }
}
