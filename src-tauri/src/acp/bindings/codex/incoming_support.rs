use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::NormalizedPayload,
    limits::LIMITS,
};

use super::{IngressError, ModelDescriptor};
use crate::acp::bindings::codex::state::{AdapterError, CodexAdapter};

pub(super) fn parse_models(
    result: &Map<String, Value>,
) -> Result<(Vec<ModelDescriptor>, Option<RequiredText<512>>), IngressError> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if data.len() > LIMITS.wire.models_or_config_entries as usize {
        return Err(IngressError::CollectionLimit);
    }
    let mut total = 0_usize;
    let mut models = Vec::with_capacity(data.len());
    for value in data {
        let model = value.as_object().ok_or(IngressError::Malformed)?;
        let id = text(model, "id", 512)?;
        let name = text(model, "displayName", 512)?;
        total = total.saturating_add(id.len()).saturating_add(name.len());
        if total > LIMITS.wire.models_or_config_bytes as usize
            || models
                .iter()
                .any(|existing: &ModelDescriptor| existing.id.as_str() == id)
        {
            return Err(IngressError::CollectionLimit);
        }
        models.push(ModelDescriptor {
            id: RequiredText::new("modelId", id.to_owned()).map_err(content)?,
            name: RequiredText::new("modelName", name.to_owned()).map_err(content)?,
            is_default: model
                .get("isDefault")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let next_cursor = result
        .get("nextCursor")
        .filter(|value| !value.is_null())
        .map(|value| {
            let cursor = value.as_str().ok_or(IngressError::Malformed)?;
            if cursor.is_empty() || cursor.len() > LIMITS.wire.page_cursor_bytes as usize {
                return Err(IngressError::CollectionLimit);
            }
            RequiredText::new("modelCursor", cursor.to_owned()).map_err(content)
        })
        .transpose()?;
    Ok((models, next_cursor))
}

pub(super) fn expect_thread_and_turn<'a>(
    adapter: &CodexAdapter,
    params: &'a Map<String, Value>,
) -> Result<&'a str, IngressError> {
    adapter
        .expect_thread(text(params, "threadId", 512)?)
        .map_err(identity)?;
    let turn = params
        .get("turnId")
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("turn")
                .and_then(Value::as_object)
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
        })
        .ok_or(IngressError::Malformed)?;
    adapter.expect_turn(turn).map_err(identity)?;
    Ok(turn)
}

pub(super) fn parse_usage(params: &Map<String, Value>) -> Result<NormalizedPayload, IngressError> {
    let last = params
        .get("tokenUsage")
        .and_then(Value::as_object)
        .and_then(|usage| usage.get("last"))
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let input_tokens = last
        .get("inputTokens")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    let output_tokens = last
        .get("outputTokens")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    Ok(NormalizedPayload::Usage {
        input_tokens,
        output_tokens,
    })
}

pub(in crate::acp::bindings::codex) fn validate_shape(value: &Value) -> Result<(), IngressError> {
    let mut nodes = 0_u64;
    fn visit(value: &Value, depth: u64, nodes: &mut u64) -> bool {
        *nodes = nodes.saturating_add(1);
        if depth > LIMITS.wire.json_depth || *nodes > LIMITS.wire.json_nodes {
            return false;
        }
        match value {
            Value::Array(values) => {
                values.len() <= LIMITS.wire.array_entries as usize
                    && values.iter().all(|value| visit(value, depth + 1, nodes))
            }
            Value::Object(values) => {
                values.len() <= LIMITS.wire.object_members as usize
                    && values.iter().all(|(key, value)| {
                        key.len() <= LIMITS.wire.decoded_key_bytes as usize
                            && visit(value, depth + 1, nodes)
                    })
            }
            _ => true,
        }
    }
    if value.is_array() || !visit(value, 0, &mut nodes) {
        Err(IngressError::Malformed)
    } else {
        Ok(())
    }
}

pub(super) fn text<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<&'a str, IngressError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(IngressError::Malformed)?;
    if value.is_empty() || value.len() > max || value.contains('\0') {
        Err(IngressError::Malformed)
    } else {
        Ok(value)
    }
}

pub(super) fn safe_text(
    value: Option<&Value>,
    max: usize,
    field: &'static str,
) -> Result<SafeText<262144>, IngressError> {
    let value = value
        .and_then(Value::as_str)
        .ok_or(IngressError::Malformed)?;
    if value.len() > max {
        return Err(IngressError::Content);
    }
    SafeText::new(field, sanitize(value)).map_err(content)
}

pub(in crate::acp::bindings::codex) fn sanitize(value: &str) -> String {
    let mut output = String::with_capacity(value.len().min(262_144));
    let mut redact_next = false;
    for part in value.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let whitespace = &part[token.len()..];
        let bare = token.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
            )
        });
        let bearer = bare.eq_ignore_ascii_case("bearer");
        let embedded_bearer = bare
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer:"));
        if redact_next || embedded_bearer || bare.contains("sk-") {
            output.push_str("[redacted]");
        } else if contains_absolute_path(token) {
            output.push_str("[path]");
        } else {
            output.push_str(token);
        }
        output.push_str(whitespace);
        redact_next = bearer;
    }
    output
}

fn contains_absolute_path(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower.contains("file:///") || lower.contains("file://\\\\") {
        return true;
    }
    let bytes = token.as_bytes();
    for index in 0..bytes.len() {
        let boundary = index == 0
            || matches!(
                bytes[index - 1],
                b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'<' | b'=' | b',' | b';'
            );
        if !boundary {
            continue;
        }
        let rest = &bytes[index..];
        if rest.starts_with(b"\\\\")
            || (rest.starts_with(b"/") && !rest.starts_with(b"//"))
            || (rest.len() >= 3
                && rest[0].is_ascii_alphabetic()
                && rest[1] == b':'
                && matches!(rest[2], b'\\' | b'/'))
        {
            return true;
        }
    }
    false
}

pub(super) fn error_payload(
    code: &str,
    message: &str,
    retryable: bool,
) -> Result<NormalizedPayload, IngressError> {
    if message.len() > LIMITS.wire.error_bytes as usize {
        return Err(IngressError::Content);
    }
    Ok(NormalizedPayload::Error {
        code: SafeText::new("errorCode", code).map_err(content)?,
        message: SafeText::new("errorMessage", sanitize(message)).map_err(content)?,
        retryable,
    })
}

pub(super) fn unsupported(kind: &str, summary: &str) -> Result<NormalizedPayload, IngressError> {
    Ok(NormalizedPayload::Unsupported {
        external_kind: SafeText::new("externalKind", sanitize(kind)).map_err(content)?,
        summary: SafeText::new("unsupportedSummary", summary).map_err(content)?,
    })
}

pub(super) fn request_id_text(value: &Value) -> Result<String, IngressError> {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return Err(IngressError::Malformed),
    };
    if value.is_empty() || value.len() > 512 {
        Err(IngressError::Malformed)
    } else {
        Ok(value)
    }
}

pub(super) fn identity(_: AdapterError) -> IngressError {
    IngressError::Identity
}

pub(super) fn capability(_: AdapterError) -> IngressError {
    IngressError::CapabilityUnavailable
}

pub(super) fn permission(_: AdapterError) -> IngressError {
    IngressError::PermissionTeardownRequired
}

pub(super) fn content(_: crate::acp::contract::ContractError) -> IngressError {
    IngressError::Content
}
