use super::*;
use serde_json::Value;

pub(super) fn redact_frame_value(value: Value) -> Value {
    match value {
        Value::String(value) => Value::String(format!("<redacted:{}>", value.len())),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_frame_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let value = if matches!(key.as_str(), "method" | "status") {
                        value
                    } else {
                        redact_frame_value(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        value => value,
    }
}

impl<A, J, R, T, C, L> ProcessAdapterDriver<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(super) fn log_rejected_inbound_frame(
        &self,
        chunk: &[u8],
        error: &crate::acp::runtime::AdapterPumpError,
    ) {
        const DIAGNOSTIC_BYTES: usize = 1024;

        let truncated = chunk.len() > DIAGNOSTIC_BYTES;
        let preview = &chunk[..chunk.len().min(DIAGNOSTIC_BYTES)];
        let parsed = serde_json::from_slice::<Value>(preview).ok();
        let method = parsed
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|object| object.get("method"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let id = parsed
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|object| object.get("id"))
            .and_then(Value::as_u64);
        let rpc_error_code = parsed
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|object| object.get("error"))
            .and_then(Value::as_object)
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        let frame_json = parsed
            .as_ref()
            .map(|value| redact_frame_value(value.clone()))
            .and_then(|value| serde_json::to_string(&value).ok())
            .unwrap_or_else(|| format!("<non-json-or-fragment bytes={}>", chunk.len()));
        tracing::warn!(
            activation_checkpoint = "process_driver.ingress.reject",
            ?error,
            json_rpc_method = method,
            json_rpc_id = id,
            json_rpc_error_code = rpc_error_code,
            frame_json = %frame_json,
            frame_truncated = truncated,
            "ACP process driver rejected inbound frame"
        );
    }
}
