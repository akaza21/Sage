use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Number, Value};

use crate::acp::limits::LIMITS;

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

#[path = "wire_text.rs"]
mod text;
pub(super) use text::{normalized_text, sanitized_command_output};

pub(super) fn opaque_id(value: Option<&Value>) -> Result<String, WireError> {
    let value = match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => return Err(WireError::Identity),
    };
    if !text::valid_opaque_id(&value) {
        return Err(WireError::Identity);
    }
    Ok(value)
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
