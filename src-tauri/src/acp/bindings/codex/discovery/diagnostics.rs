use serde_json::Value;

use crate::acp::transport::Frame;

pub(super) fn log_initialize_response(frame: &Frame) {
    let Ok(value) = serde_json::from_slice::<Value>(frame.as_bytes()) else {
        tracing::warn!("Codex discovery initialize response was not JSON");
        return;
    };
    let Some(object) = value.as_object() else {
        tracing::warn!("Codex discovery initialize response was not an object");
        return;
    };
    let response_id = object.get("id").and_then(Value::as_u64);
    if let Some(error) = object.get("error").and_then(Value::as_object) {
        tracing::warn!(
            ?response_id,
            json_rpc_error_code = ?error.get("code"),
            json_rpc_error_message = ?error.get("message"),
            "Codex rejected the discovery initialize request"
        );
        return;
    }
    let Some(result) = object.get("result").and_then(Value::as_object) else {
        tracing::warn!(
            ?response_id,
            "Codex discovery initialize response had neither result nor error"
        );
        return;
    };
    tracing::info!(
        ?response_id,
        result_fields = ?result.keys().collect::<Vec<_>>(),
        protocol_version = ?result.get("protocolVersion"),
        version = ?result.get("version"),
        agent_info = ?result.get("agentInfo"),
        codex_home = ?result.get("codexHome"),
        user_agent = ?result.get("userAgent"),
        platform_family = ?result.get("platformFamily"),
        platform_os = ?result.get("platformOs"),
        "Codex discovery initialize response received"
    );
}
