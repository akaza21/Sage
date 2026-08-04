use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Number, Value};

use crate::acp::limits::LIMITS;

pub(super) fn decode_frame(raw: &[u8]) -> Result<Value, WireError> {
    if raw.is_empty() || raw.len() as u64 > LIMITS.wire.frame_bytes || raw.contains(&0) {
        return Err(WireError::ByteLimitOrNul);
    }
    let text = std::str::from_utf8(raw).map_err(|_| WireError::Utf8)?;
    preflight_nesting(text)?;
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
        formatter.write_str("a JSON value without duplicate decoded keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
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

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
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

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = Map::new();
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if key.len() as u64 > LIMITS.wire.decoded_key_bytes
                || keys.len() as u64 >= LIMITS.wire.object_members
            {
                return Err(de::Error::custom("object key/member limit exceeded"));
            }
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom("duplicate decoded object key"));
            }
            let value = map.next_value::<UniqueValue>()?;
            values.insert(key, value.0);
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

fn preflight_nesting(text: &str) -> Result<(), WireError> {
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
            '}' | ']' => {
                depth = depth.checked_sub(1).ok_or(WireError::Json)?;
            }
            _ => {}
        }
    }
    if depth != 0 || in_string || escaped {
        return Err(WireError::Json);
    }
    Ok(())
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
        Value::Object(object) => {
            if object.len() as u64 > LIMITS.wire.object_members {
                return Err(WireError::ObjectMembers);
            }
            for (key, child) in object {
                if key.len() as u64 > LIMITS.wire.decoded_key_bytes {
                    return Err(WireError::KeyBytes);
                }
                validate_value(child, depth + 1, nodes)?;
            }
        }
        Value::Array(array) => {
            if array.len() as u64 > LIMITS.wire.array_entries {
                return Err(WireError::ArrayEntries);
            }
            for child in array {
                validate_value(child, depth + 1, nodes)?;
            }
        }
        Value::Number(number)
            if number.to_string().len() as u64 > LIMITS.wire.numeric_token_bytes =>
        {
            return Err(WireError::NumericBytes);
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum WireError {
    #[error("Claude ACP frame is empty, oversized, or contains NUL")]
    ByteLimitOrNul,
    #[error("Claude ACP frame is not UTF-8")]
    Utf8,
    #[error("Claude ACP frame is not one valid JSON value")]
    Json,
    #[error("Claude ACP frame must be one object")]
    TopLevelObject,
    #[error("Claude ACP JSON depth exceeded its limit")]
    Depth,
    #[error("Claude ACP JSON node count exceeded its limit")]
    Nodes,
    #[error("Claude ACP JSON object member count exceeded its limit")]
    ObjectMembers,
    #[error("Claude ACP JSON array entry count exceeded its limit")]
    ArrayEntries,
    #[error("Claude ACP decoded key exceeded its byte limit")]
    KeyBytes,
    #[error("Claude ACP numeric token exceeded its byte limit")]
    NumericBytes,
}
