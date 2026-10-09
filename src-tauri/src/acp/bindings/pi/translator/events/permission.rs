use super::super::{PendingUiRequest, PiTranslator};
use crate::acp::bindings::pi::{
    AdapterError, AdapterOutput, decode::external_id, protocol::is_allow_option,
};
use serde_json::Value;

impl PiTranslator {
    pub(super) fn extension_request(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        self.live_turn_any_phase()?;
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            method,
            "notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text"
        ) {
            return Ok(vec![AdapterOutput::Absorbed]);
        }
        self.live_turn()?;
        if !matches!(method, "confirm" | "select") {
            return Ok(vec![AdapterOutput::Unsupported {
                kind: "extension_ui_request".into(),
            }]);
        }
        let id = external_id(value.get("id"))?.ok_or(AdapterError::Malformed)?;
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
                return Ok(vec![AdapterOutput::Unsupported {
                    kind: "extension_ui_request".into(),
                }]);
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
