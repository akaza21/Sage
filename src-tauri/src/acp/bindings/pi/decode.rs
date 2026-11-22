use serde_json::Value;

use super::protocol::AdapterError;
use crate::acp::bindings::standard_acp::sanitize::sanitized_content_output;
use crate::acp::redaction::text as redaction;
use crate::acp::transport::CommandWirePath;

pub(super) const FRAME_BYTES: usize = 1_048_576;
pub(super) const EXTERNAL_ID_BYTES: usize = 512;
/// A prompt's text with its attached files inlined: everything one dispatch
/// admits, plus each file's `<context>` wrapper.
pub(super) const PROMPT_BYTES: usize = sage_core::limits::prompt_files::DISPATCH_BYTES
    + sage_core::limits::prompt_files::FILE_COUNT * 512;
pub(super) const ITEM_BYTES: usize = 262_144;
pub(super) const MODEL_ENTRIES: usize = 128;
pub(super) const MODEL_BYTES: usize = 262_144;
pub(super) const MODEL_NAME_BYTES: usize = 8_192;
pub(super) const PAGE_ENTRIES: usize = 256;
pub(super) const PAGE_BYTES: usize = 2_097_152;

const JSON_DEPTH: usize = 32;
const JSON_NODES: usize = 16_384;
const OBJECT_MEMBERS: usize = 256;
const KEY_BYTES: usize = 256;

#[derive(Debug)]
pub(super) struct Response {
    pub id: String,
    pub command: String,
    pub success: bool,
    pub data: Value,
}

#[derive(Debug)]
pub(super) enum Record {
    Response(Response),
    Event { kind: String, value: Value },
}

pub(super) fn decode_record(bytes: &[u8]) -> Result<Record, AdapterError> {
    decode_record_with_optional_request(bytes, |_| None)
}

/// Older Pi versions omit correlation IDs only for unknown commands. The
/// runtime may supply the sole pending optional-discovery request for that
/// method; every other response still requires its wire ID.
pub(super) fn decode_record_with_optional_request<'a>(
    bytes: &[u8],
    optional_request_id: impl FnOnce(&str) -> Option<&'a str>,
) -> Result<Record, AdapterError> {
    if bytes.len() > FRAME_BYTES + 2 {
        return Err(AdapterError::Limit("frame bytes"));
    }
    let mut record = bytes;
    if record.last() == Some(&b'\n') {
        record = &record[..record.len() - 1];
        if record.last() == Some(&b'\r') {
            record = &record[..record.len() - 1];
        }
    }
    if record.len() > FRAME_BYTES {
        return Err(AdapterError::Limit("frame bytes"));
    }
    if record.is_empty() || record.iter().any(|byte| *byte == b'\n' || *byte == b'\r') {
        return Err(AdapterError::Malformed);
    }
    let text = std::str::from_utf8(record).map_err(|_| AdapterError::Malformed)?;
    let mut value: Value = serde_json::from_str(text).map_err(|_| AdapterError::Malformed)?;
    match validate_shape(&value, 0, &mut 0, CommandWirePath::Other) {
        Err(AdapterError::Limit("JSON array entries")) => {
            validate_shape(&value, 0, &mut 0, CommandWirePath::for_frame(record))?;
        }
        result => result?,
    }
    let object = value.as_object_mut().ok_or(AdapterError::Malformed)?;
    let kind = bounded_field(object.get("type"), "record type", 128)?;
    if kind == "response" {
        if !object.contains_key("id")
            && object.get("success").and_then(Value::as_bool) == Some(false)
            && let Some(command) = object.get("command").and_then(Value::as_str)
            && let Some(id) = optional_request_id(command)
        {
            object.insert("id".into(), Value::String(id.to_owned()));
        }
        return Ok(Record::Response(Response {
            id: bounded_field(object.get("id"), "command ID", EXTERNAL_ID_BYTES)?,
            command: bounded_field(object.get("command"), "command", 128)?,
            success: object
                .get("success")
                .and_then(Value::as_bool)
                .ok_or(AdapterError::Malformed)?,
            data: object.remove("data").unwrap_or(Value::Null),
        }));
    }
    if kind.len() > 128 || kind.is_empty() || kind.contains('\0') {
        return Err(AdapterError::Limit("event type"));
    }
    Ok(Record::Event { kind, value })
}

pub(super) fn bounded_string(
    value: Option<&Value>,
    limit: usize,
) -> Result<Option<String>, AdapterError> {
    Ok(bounded_text(value, limit)?.map(|text| redact_structured_text(&text)))
}

/// A string as Pi sent it, unredacted, for text the runtime assembles from
/// deltas and redacts whole.
pub(super) fn bounded_text(
    value: Option<&Value>,
    limit: usize,
) -> Result<Option<String>, AdapterError> {
    let Some(value) = value else { return Ok(None) };
    let text = value.as_str().ok_or(AdapterError::Malformed)?;
    if text.len() > limit || text.contains('\0') {
        return Err(AdapterError::Limit("text bytes"));
    }
    Ok(Some(text.to_owned()))
}

pub(super) fn external_id(value: Option<&Value>) -> Result<Option<String>, AdapterError> {
    opaque_id(value, EXTERNAL_ID_BYTES)
}

pub(super) fn opaque_id(
    value: Option<&Value>,
    limit: usize,
) -> Result<Option<String>, AdapterError> {
    let Some(value) = value else { return Ok(None) };
    let text = value.as_str().ok_or(AdapterError::Malformed)?;
    validate_opaque_text(text, limit)?;
    Ok(Some(text.to_owned()))
}

pub(super) fn validate_opaque_text(text: &str, limit: usize) -> Result<(), AdapterError> {
    if text.is_empty()
        || text.len() > limit
        || text.contains('\0')
        || text.split_whitespace().any(is_sensitive_token)
    {
        return Err(AdapterError::Limit("opaque identity"));
    }
    Ok(())
}

/// Redacts harness prose as every harness's replies are.
pub(super) fn redact_structured_text(text: &str) -> String {
    redaction::reply_text(text)
}

/// Tool output, which keeps absolute paths private as every harness's does.
pub(super) fn redact_tool_output(text: &str) -> String {
    sanitized_content_output(text).unwrap_or_else(|_| "[redacted]".to_owned())
}

fn is_sensitive_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    token.starts_with('/')
        || token.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
        || token.starts_with("sk-")
}

fn bounded_field(
    value: Option<&Value>,
    name: &'static str,
    limit: usize,
) -> Result<String, AdapterError> {
    let value = value
        .and_then(Value::as_str)
        .ok_or(AdapterError::Malformed)?;
    if value.is_empty() || value.len() > limit || value.contains('\0') {
        return Err(AdapterError::Limit(name));
    }
    Ok(value.to_owned())
}

fn validate_shape(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    path: CommandWirePath,
) -> Result<(), AdapterError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or(AdapterError::Limit("JSON nodes"))?;
    if depth > JSON_DEPTH || *nodes > JSON_NODES {
        return Err(AdapterError::Limit("JSON shape"));
    }
    match value {
        Value::Array(values) => {
            if values.len() as u64 > path.array_limit() {
                return Err(AdapterError::Limit("JSON array entries"));
            }
            for value in values {
                validate_shape(value, depth + 1, nodes, CommandWirePath::Other)?;
            }
        }
        Value::Object(values) => {
            if values.len() > OBJECT_MEMBERS
                || values
                    .keys()
                    .any(|key| key.len() > KEY_BYTES || key.contains('\0'))
            {
                return Err(AdapterError::Limit("JSON object"));
            }
            for (key, value) in values {
                validate_shape(value, depth + 1, nodes, path.child(key))?;
            }
        }
        _ => {}
    }
    Ok(())
}
