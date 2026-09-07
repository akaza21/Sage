use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Number, Value};

use crate::acp::{
    contract::SafeText,
    limits::{COMMAND_OUTPUT_REDACTION_BYTES, LIMITS},
};

pub(super) fn decode_frame(raw: &[u8]) -> Result<Value, WireError> {
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    if raw.is_empty()
        || raw.len() as u64 > LIMITS.wire.frame_bytes
        || raw.contains(&0)
        || raw.contains(&b'\n')
        || raw.contains(&b'\r')
    {
        return Err(WireError::ByteLimitOrDelimiter);
    }
    let text = std::str::from_utf8(raw).map_err(|_| WireError::Utf8)?;
    preflight_depth(text)?;
    let value = serde_json::from_slice::<UniqueValue>(raw)
        .map_err(|_| WireError::Json)?
        .0;
    if !value.is_object() {
        return Err(WireError::TopLevelObject);
    }
    let mut nodes = 0_u64;
    validate_value(&value, 1, &mut nodes)?;
    Ok(value)
}

pub(super) fn normalized_text<const MAX: usize>(
    field: &'static str,
    text: &str,
) -> Result<SafeText<MAX>, WireError> {
    if text.len() > MAX || text.contains('\0') {
        return Err(WireError::TextLimit);
    }
    let mut result = String::with_capacity(text.len());
    let mut redact_next = false;
    for part in text.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let suffix = &part[token.len()..];
        let bare = token.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        let bearer = bare.eq_ignore_ascii_case("bearer");
        if redact_next || is_sensitive(bare) {
            result.push_str("[redacted]");
            result.push_str(suffix);
        } else {
            result.push_str(part);
        }
        redact_next = bearer;
        if result.len() > MAX {
            return Err(WireError::TextLimit);
        }
    }
    SafeText::new(field, result).map_err(|_| WireError::TextLimit)
}

/// Sanitizes one already frame-bounded command-output value before its display
/// suffix is retained. General `normalized_text` callers retain their strict
/// `SafeText` maximum; oversized command output is valid and is capped later.
pub(super) fn sanitized_command_output(input: &str) -> Result<String, WireError> {
    let maximum_input = LIMITS.wire.frame_bytes as usize;
    let maximum_output = maximum_input.saturating_mul(COMMAND_OUTPUT_REDACTION_BYTES);
    if input.len() > maximum_input || input.contains('\0') {
        return Err(WireError::TextLimit);
    }

    let mut result = String::new();
    let mut redact_next = false;
    for part in input.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let suffix = &part[token.len()..];
        if token.is_empty() {
            append_command_output(&mut result, suffix, maximum_output)?;
            continue;
        }
        let bare = token.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        let replacement =
            if redact_next || is_sensitive(bare) || contains_command_absolute_path(token) {
                "[redacted]"
            } else {
                token
            };
        append_command_output(&mut result, replacement, maximum_output)?;
        append_command_output(&mut result, suffix, maximum_output)?;
        redact_next = is_bearer_marker(bare);
    }
    Ok(result)
}

fn is_bearer_marker(value: &str) -> bool {
    value.eq_ignore_ascii_case("bearer") || value.eq_ignore_ascii_case("bearer:")
}

fn append_command_output(
    output: &mut String,
    value: &str,
    maximum: usize,
) -> Result<(), WireError> {
    let next = output
        .len()
        .checked_add(value.len())
        .ok_or(WireError::TextLimit)?;
    if next > maximum {
        return Err(WireError::TextLimit);
    }
    output
        .try_reserve(value.len())
        .map_err(|_| WireError::TextLimit)?;
    output.push_str(value);
    Ok(())
}

fn contains_command_absolute_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("file:///") || lower.contains("file://\\\\") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.iter().enumerate().any(|(index, _)| {
        let boundary = index == 0
            || matches!(
                bytes[index - 1],
                b'"' | b'\'' | b'`' | b'(' | b'[' | b'{' | b'<' | b'=' | b',' | b';'
            );
        let rest = &bytes[index..];
        boundary
            && (rest.starts_with(b"\\\\")
                || (rest.starts_with(b"/") && !rest.starts_with(b"//"))
                || (rest.len() >= 3
                    && rest[0].is_ascii_alphabetic()
                    && rest[1] == b':'
                    && matches!(rest[2], b'\\' | b'/')))
    })
}

pub(super) fn opaque_id(value: Option<&Value>) -> Result<String, WireError> {
    let value = match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => return Err(WireError::Identity),
    };
    if value.is_empty()
        || value.len() as u64 > LIMITS.wire.external_id_bytes
        || value.contains('\0')
        || value.split_whitespace().any(is_sensitive)
    {
        return Err(WireError::Identity);
    }
    Ok(value)
}

fn is_sensitive(token: &str) -> bool {
    let bytes = token.as_bytes();
    token.starts_with('/')
        || token.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
        || token.starts_with("sk-")
        || token.starts_with("ghp_")
        || token.starts_with("xoxb-")
        || token.starts_with("AKIA")
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> de::Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value with unique decoded keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(value.into())))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(value.into())))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueValue)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.visit_string(value.to_owned())
    }
    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }
    fn visit_seq<A: de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::with_capacity(
            sequence
                .size_hint()
                .unwrap_or(0)
                .min(LIMITS.wire.array_entries as usize),
        );
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            if values.len() as u64 >= LIMITS.wire.array_entries {
                return Err(de::Error::custom("array entry limit exceeded"));
            }
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }
    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = Map::new();
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if key.len() as u64 > LIMITS.wire.decoded_key_bytes
                || keys.len() as u64 >= LIMITS.wire.object_members
                || !keys.insert(key.clone())
            {
                return Err(de::Error::custom("object key/member limit exceeded"));
            }
            values.insert(key, map.next_value::<UniqueValue>()?.0);
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

fn preflight_depth(text: &str) -> Result<(), WireError> {
    let mut depth = 0_u64;
    let mut in_string = false;
    let mut escaped = false;
    for character in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => {
                depth = depth.checked_add(1).ok_or(WireError::Depth)?;
                if depth > LIMITS.wire.json_depth {
                    return Err(WireError::Depth);
                }
            }
            '}' | ']' => depth = depth.checked_sub(1).ok_or(WireError::Json)?,
            _ => {}
        }
    }
    if depth == 0 && !in_string && !escaped {
        Ok(())
    } else {
        Err(WireError::Json)
    }
}

fn validate_value(value: &Value, depth: u64, nodes: &mut u64) -> Result<(), WireError> {
    if depth > LIMITS.wire.json_depth {
        return Err(WireError::Depth);
    }
    *nodes = nodes.checked_add(1).ok_or(WireError::Nodes)?;
    if *nodes > LIMITS.wire.json_nodes {
        return Err(WireError::Nodes);
    }
    match value {
        Value::Object(values) => {
            if values.len() as u64 > LIMITS.wire.object_members {
                return Err(WireError::Members);
            }
            for (key, child) in values {
                if key.len() as u64 > LIMITS.wire.decoded_key_bytes {
                    return Err(WireError::Key);
                }
                validate_value(child, depth + 1, nodes)?;
            }
        }
        Value::Array(values) => {
            if values.len() as u64 > LIMITS.wire.array_entries {
                return Err(WireError::Array);
            }
            for child in values {
                validate_value(child, depth + 1, nodes)?;
            }
        }
        Value::Number(number)
            if number.to_string().len() as u64 > LIMITS.wire.numeric_token_bytes =>
        {
            return Err(WireError::Number);
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum WireError {
    #[error("OMP ACP frame is empty, oversized, contains NUL, or contains an embedded delimiter")]
    ByteLimitOrDelimiter,
    #[error("OMP ACP frame is not UTF-8")]
    Utf8,
    #[error("OMP ACP frame is not one valid JSON value")]
    Json,
    #[error("OMP ACP frame must be one object")]
    TopLevelObject,
    #[error("OMP ACP JSON depth exceeded its limit")]
    Depth,
    #[error("OMP ACP JSON node count exceeded its limit")]
    Nodes,
    #[error("OMP ACP JSON object member count exceeded its limit")]
    Members,
    #[error("OMP ACP JSON array entry count exceeded its limit")]
    Array,
    #[error("OMP ACP JSON key exceeded its limit")]
    Key,
    #[error("OMP ACP numeric token exceeded its limit")]
    Number,
    #[error("OMP ACP identity is missing, oversized, or sensitive")]
    Identity,
    #[error("OMP ACP text exceeded its limit")]
    TextLimit,
}
