use serde_json::{Map, Value};

use sage_acp_protocol::{
    contract::SafeText,
    intent::{NormalizedPayload, SafeResourceLocation},
};
use sage_core::limits::LIMITS;

use super::{IngressError, ResourceMapper};
use crate::acp::bindings::codex::state::{AdapterError, CodexAdapter};

#[path = "support/sanitize.rs"]
mod sanitize;
pub(in crate::acp::bindings::codex) use sanitize::{
    command_output_token, sanitize, sanitize_hiding_paths, sanitize_tool_text,
    sanitized_command_line, sanitized_command_output,
};

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
    validate_shape_with_catalog(value, crate::acp::transport::CommandWirePath::Other)
}

pub(super) fn validate_shape_with_catalog(
    value: &Value,
    path: crate::acp::transport::CommandWirePath,
) -> Result<(), IngressError> {
    use crate::acp::transport::CommandWirePath;
    let mut nodes = 0_u64;
    fn visit(value: &Value, depth: u64, nodes: &mut u64, path: CommandWirePath) -> bool {
        *nodes = nodes.saturating_add(1);
        if depth > LIMITS.wire.json_depth || *nodes > LIMITS.wire.json_nodes {
            return false;
        }
        match value {
            Value::Array(values) => {
                values.len() <= path.array_limit() as usize
                    && values
                        .iter()
                        .all(|value| visit(value, depth + 1, nodes, path.element()))
            }
            Value::Object(values) => {
                values.len() <= LIMITS.wire.object_members as usize
                    && values.iter().all(|(key, value)| {
                        key.len() <= LIMITS.wire.decoded_key_bytes as usize
                            && visit(value, depth + 1, nodes, path.child(key))
                    })
            }
            _ => true,
        }
    }
    if value.is_array() || !visit(value, 0, &mut nodes, path) {
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

/// The Codex turn a `turn/started` names. An inline review starts a nested
/// turn whose `rootTurnId` is the turn `review/start` answers, so that root
/// names the Sage Turn when `root_names_turn` admits it. Any other root, such
/// as an earlier finished turn, leaves the started turn as its own.
pub(super) fn started_turn_id(
    turn: &Map<String, Value>,
    root_names_turn: impl FnOnce(&str) -> bool,
) -> Result<&str, IngressError> {
    let id = text(turn, "id", 512)?;
    match turn.get("rootTurnId") {
        None | Some(Value::Null) => Ok(id),
        Some(_) => {
            let root = text(turn, "rootTurnId", 512)?;
            Ok(if root_names_turn(root) { root } else { id })
        }
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

pub(super) fn error_payload(
    code: &str,
    message: &str,
    retryable: bool,
) -> Result<NormalizedPayload, IngressError> {
    if message.len() > LIMITS.wire.error_bytes as usize {
        return Err(IngressError::Content);
    }
    Ok(NormalizedPayload::Error {
        kind: None,
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

pub(super) fn content(_: sage_acp_protocol::contract::ContractError) -> IngressError {
    IngressError::Content
}

#[cfg(test)]
#[path = "support/tests.rs"]
mod tests;
