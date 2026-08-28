use serde_json::Value;

pub(super) fn safe_frame_diagnostic(chunk: &[u8]) -> (Option<String>, String, String) {
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
    let json = parsed
        .map(redact_frame_value)
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_else(|| format!("<non-json-or-fragment bytes={}>", chunk.len()));
    let json = if truncated {
        format!("{json}<truncated>")
    } else {
        json
    };
    let hex = json
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    (method, json, hex)
}

fn redact_frame_value(value: Value) -> Value {
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

#[cfg(test)]
mod tests {
    use super::safe_frame_diagnostic;

    #[test]
    fn preserves_mcp_startup_status_while_redacting_other_strings() {
        let (_, json, _) = safe_frame_diagnostic(
            br#"{"method":"mcpServer/startupStatus/updated","params":{"name":"developer","status":"completed","threadId":"thread-1"}}"#,
        );

        assert!(json.contains(r#""status":"completed""#));
        assert!(json.contains(r#""name":"<redacted:9>""#));
        assert!(json.contains(r#""threadId":"<redacted:8>""#));
    }
}
