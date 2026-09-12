//! Bounded permission-option parsing for Claude ingress.

use super::super::*;

pub(super) fn permission_allow_once_option(value: &Value) -> Result<String, AdapterRuntimeError> {
    let options = value
        .get("params")
        .and_then(Value::as_object)
        .and_then(|params| params.get("options"))
        .and_then(Value::as_array)
        .ok_or(AdapterRuntimeError::Frame)?;
    options
        .iter()
        .take(8)
        .find_map(|option| {
            let option_id = option.get("optionId")?.as_str().map(str::to_owned)?;
            if option_id.len() > 64 {
                return None;
            }
            let kind = option
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if kind == "allow_once" {
                return Some(option_id);
            }
            let name = option
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name.len() > 64 {
                return None;
            }
            let name = name.to_ascii_lowercase();
            (kind.is_empty() && matches!(name.as_str(), "allow" | "allow once"))
                .then_some(option_id)
        })
        .ok_or(AdapterRuntimeError::Frame)
}
