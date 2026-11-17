use crate::acp::transport::Frame;

pub(crate) fn is_available_commands_update(frame: &Frame, expected_session_id: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(frame.as_bytes()) else {
        return false;
    };
    let Some(params) = value.get("params") else {
        return false;
    };
    value.get("jsonrpc").and_then(serde_json::Value::as_str) == Some("2.0")
        && value.get("method").and_then(serde_json::Value::as_str) == Some("session/update")
        && value.get("id").is_none()
        && value.get("result").is_none()
        && value.get("error").is_none()
        && params.get("sessionId").and_then(serde_json::Value::as_str) == Some(expected_session_id)
        && params
            .get("update")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|update| {
                update
                    .get("sessionUpdate")
                    .and_then(serde_json::Value::as_str)
                    == Some("available_commands_update")
                    && update
                        .get("availableCommands")
                        .is_some_and(serde_json::Value::is_array)
            })
}
