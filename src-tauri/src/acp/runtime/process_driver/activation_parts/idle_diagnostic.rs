pub(super) fn log_idle_frame_rejection(frame: &crate::acp::transport::Frame, reason: &'static str) {
    let value = serde_json::from_slice::<serde_json::Value>(frame.as_bytes()).ok();
    let object = value.as_ref().and_then(serde_json::Value::as_object);
    let method = object
        .and_then(|object| object.get("method"))
        .and_then(serde_json::Value::as_str);
    let response_id = object
        .and_then(|object| object.get("id"))
        .and_then(serde_json::Value::as_u64);
    let error_code = object
        .and_then(|object| object.get("error"))
        .and_then(serde_json::Value::as_object)
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_i64);
    tracing::warn!(
        activation_checkpoint = "activation.idle_stdout",
        idle_frame_reason = reason,
        jsonrpc_method = ?method,
        jsonrpc_response_id = ?response_id,
        jsonrpc_error_code = ?error_code,
        "ACP idle handoff rejected inbound frame"
    );
}
