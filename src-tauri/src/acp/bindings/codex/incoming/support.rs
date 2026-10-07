use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{NormalizedPayload, SafeResourceLocation},
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
};

use super::{IngressError, ModelDescriptor, ResourceMapper};
use crate::acp::bindings::codex::state::{AdapterError, CodexAdapter};

pub(in crate::acp::bindings::codex) fn command_cwd<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
) -> Option<SafeResourceLocation> {
    let path = item.get("cwd").and_then(Value::as_str).filter(|path| {
        !path.is_empty()
            && path.len() <= LIMITS.wire.absolute_path_bytes_native_only as usize
            && !path.contains('\0')
    })?;
    Some(match mapper.map_existing(path) {
        Ok(resource) => SafeResourceLocation::Resource { resource },
        Err(reason) => SafeResourceLocation::unavailable(reason),
    })
}

pub(in crate::acp::bindings::codex) fn command_exit_code(item: &Map<String, Value>) -> Option<i32> {
    item.get("exitCode")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok())
}

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
    let context_window = params
        .get("tokenUsage")
        .and_then(Value::as_object)
        .and_then(|usage| usage.get("modelContextWindow"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0);
    Ok(NormalizedPayload::Usage {
        input_tokens,
        output_tokens,
        context_window,
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
    sanitize_with_limit(value, usize::MAX, false).unwrap_or_default()
}

pub(in crate::acp::bindings::codex) fn sanitized_command_output(
    value: &str,
) -> Result<String, IngressError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    if value.len() > maximum_input {
        return Err(IngressError::Content);
    }
    sanitize_with_limit(
        value,
        maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES),
        true,
    )
    .ok_or(IngressError::Content)
}

pub(in crate::acp::bindings::codex) fn sanitized_command_line(
    value: &str,
) -> Result<String, IngressError> {
    let flattened: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized_command_output(&flattened)?;
    let mut line = String::new();
    for token in sanitized.split_whitespace() {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(token);
    }
    Ok(line)
}

fn sanitize_with_limit(value: &str, maximum: usize, command_output: bool) -> Option<String> {
    let mut output = if command_output {
        String::new()
    } else {
        String::with_capacity(value.len().min(262_144))
    };
    let mut redact_next = false;
    for part in value.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let whitespace = &part[token.len()..];
        if command_output && token.is_empty() {
            append_command_output(&mut output, whitespace, maximum)?;
            continue;
        }
        if command_output {
            let (replacement, command_output_next) = command_output_token(token, redact_next);
            append_command_output(&mut output, replacement, maximum)?;
            append_command_output(&mut output, whitespace, maximum)?;
            redact_next = command_output_next;
        } else {
            // Keep the generic sanitizer's historical `Bearer` transition:
            // `Bearer:` redacts itself but never carries into a later token.
            let bare = token.trim_matches(command_output_trim_character);
            let embedded_bearer = bare
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer:"));
            let replacement = if redact_next || embedded_bearer || bare.contains("sk-") {
                "[redacted]"
            } else if contains_absolute_path(token) {
                "[path]"
            } else {
                token
            };
            output.push_str(replacement);
            output.push_str(whitespace);
            redact_next = bare.eq_ignore_ascii_case("bearer");
        }
    }
    Some(output)
}

/// Classifies one completed command-output token. Callers that receive output
/// in frames must keep incomplete tokens private until a whitespace boundary,
/// then pass the complete token and the carried Bearer state here.
pub(in crate::acp::bindings::codex) fn command_output_token(
    token: &str,
    redact_next: bool,
) -> (&str, bool) {
    let bare = token.trim_matches(command_output_trim_character);
    let embedded_bearer = bare
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer:"));
    let replacement = if redact_next || embedded_bearer || bare.contains("sk-") {
        "[redacted]"
    } else if contains_absolute_path(token) {
        "[path]"
    } else {
        token
    };
    (
        replacement,
        bare.eq_ignore_ascii_case("bearer") || bare.eq_ignore_ascii_case("bearer:"),
    )
}

fn command_output_trim_character(character: char) -> bool {
    matches!(
        character,
        '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
    )
}

fn append_command_output(output: &mut String, value: &str, maximum: usize) -> Option<()> {
    let next = output.len().checked_add(value.len())?;
    if next > maximum {
        return None;
    }
    output.try_reserve(value.len()).ok()?;
    output.push_str(value);
    Some(())
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

pub(in crate::acp::bindings::codex) fn is_browse_alias(label: &str) -> bool {
    matches!(
        label,
        crate::acp::runtime::host_browse::CURATED_BROWSE_TOOL_NAME | "sage.browse"
    )
}

pub(super) fn unsupported(kind: &str, summary: &str) -> Result<NormalizedPayload, IngressError> {
    Ok(NormalizedPayload::Unsupported {
        external_kind: SafeText::new("externalKind", sanitize(kind)).map_err(content)?,
        summary: SafeText::new("unsupportedSummary", summary).map_err(content)?,
    })
}

pub(super) fn request_id_text(value: &Value) -> Result<String, IngressError> {
    let (kind, value) = match value {
        Value::String(value) => ("string", value.clone()),
        Value::Number(value) => ("number", value.to_string()),
        _ => return Err(IngressError::Malformed),
    };
    if value.is_empty() {
        return Err(IngressError::Malformed);
    }
    let value = format!("{kind}:{value}");
    if value.len() > 512 {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{IngressError, parse_usage, sanitize};
    use crate::acp::intent::NormalizedPayload;

    #[test]
    fn usage_context_window_is_known_only_for_a_positive_bounded_integer() {
        let known = json!({
            "tokenUsage": {
                "last": { "inputTokens": 3, "outputTokens": 2 },
                "modelContextWindow": 272000
            }
        });
        assert_eq!(
            parse_usage(known.as_object().unwrap()),
            Ok(NormalizedPayload::Usage {
                input_tokens: 3,
                output_tokens: 2,
                context_window: Some(272_000),
            })
        );

        let unknown = json!({
            "tokenUsage": { "last": { "inputTokens": 3, "outputTokens": 2 } }
        });
        assert_eq!(
            parse_usage(unknown.as_object().unwrap()),
            Ok(NormalizedPayload::Usage {
                input_tokens: 3,
                output_tokens: 2,
                context_window: None,
            })
        );
    }

    #[test]
    fn invalid_usage_context_windows_are_unknown_without_rejecting_usage() {
        for invalid in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(u64::from(u32::MAX) + 1),
            json!("272000"),
            json!(null),
            json!({}),
        ] {
            let params = json!({
                "tokenUsage": {
                    "last": { "inputTokens": 3, "outputTokens": 2 },
                    "modelContextWindow": invalid
                }
            });
            assert_eq!(
                parse_usage(params.as_object().unwrap()),
                Ok(NormalizedPayload::Usage {
                    input_tokens: 3,
                    output_tokens: 2,
                    context_window: None,
                })
            );
        }
    }

    #[test]
    fn malformed_usage_counts_still_fail_closed() {
        for last in [
            json!({ "outputTokens": 2 }),
            json!({ "inputTokens": 3 }),
            json!({ "inputTokens": -1, "outputTokens": 2 }),
            json!({ "inputTokens": 3, "outputTokens": "2" }),
        ] {
            let params = json!({ "tokenUsage": { "last": last } });
            assert_eq!(
                parse_usage(params.as_object().unwrap()),
                Err(IngressError::Malformed)
            );
        }
    }

    #[test]
    fn generic_bearer_colon_does_not_carry_redaction_to_the_next_token() {
        assert_eq!(sanitize("Bearer: opaque"), "[redacted] opaque");
    }
}
