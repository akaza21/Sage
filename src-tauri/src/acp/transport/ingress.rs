use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use sage_core::limits::LIMITS;

use super::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructuralError {
    ArrayTooLarge,
    Batch,
    DecodedKeyTooLarge,
    Depth,
    DuplicateKey,
    MethodTooLarge,
    MethodType,
    Nodes,
    NumericTokenTooLarge,
    ObjectTooLarge,
    RootType,
    Syntax,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Object,
    Other,
}

struct Budget {
    nodes: u64,
}

pub(crate) fn validate_structure(frame: &Frame) -> Result<(), StructuralError> {
    validate_numeric_tokens(frame.as_bytes())?;
    let mut budget = Budget { nodes: 0 };
    let mut deserializer = serde_json::Deserializer::from_slice(frame.as_bytes());
    let kind = NodeSeed {
        budget: &mut budget,
        depth: 0,
        method: false,
        count_node: false,
    }
    .deserialize(&mut deserializer)
    .map_err(map_error)?;
    deserializer.end().map_err(|_| StructuralError::Syntax)?;
    if kind != Kind::Object {
        return Err(if frame.as_bytes().first() == Some(&b'[') {
            StructuralError::Batch
        } else {
            StructuralError::RootType
        });
    }
    Ok(())
}

fn validate_numeric_tokens(bytes: &[u8]) -> Result<(), StructuralError> {
    let limit = usize::try_from(LIMITS.wire.numeric_token_bytes).unwrap_or(usize::MAX);
    let mut index = 0;
    let mut quoted = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            quoted = true;
            index += 1;
            continue;
        }
        if byte == b'-' || byte.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && !matches!(
                    bytes[index],
                    b',' | b']' | b'}' | b' ' | b'\t' | b'\r' | b'\n'
                )
            {
                index += 1;
            }
            if index - start > limit {
                return Err(StructuralError::NumericTokenTooLarge);
            }
            continue;
        }
        index += 1;
    }
    Ok(())
}

struct NodeSeed<'a> {
    budget: &'a mut Budget,
    depth: u64,
    method: bool,
    count_node: bool,
}

impl<'de> DeserializeSeed<'de> for NodeSeed<'_> {
    type Value = Kind;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        if self.depth > LIMITS.wire.json_depth {
            return Err(de::Error::custom("depth"));
        }
        if self.count_node {
            self.budget.nodes += 1;
            if self.budget.nodes > LIMITS.wire.json_nodes {
                return Err(de::Error::custom("nodes"));
            }
        }
        deserializer.deserialize_any(NodeVisitor {
            budget: self.budget,
            depth: self.depth,
            method: self.method,
        })
    }
}

struct NodeVisitor<'a> {
    budget: &'a mut Budget,
    depth: u64,
    method: bool,
}

impl<'de> Visitor<'de> for NodeVisitor<'_> {
    type Value = Kind;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        if self.method {
            return Err(de::Error::custom("method-type"));
        }
        let mut count = 0_u64;
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            count += 1;
            if count > LIMITS.wire.object_members {
                return Err(de::Error::custom("object-members"));
            }
            if key.len() as u64 > LIMITS.wire.decoded_key_bytes {
                return Err(de::Error::custom("decoded-key"));
            }
            let is_method = self.depth == 0 && key == "method";
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate-key"));
            }
            map.next_value_seed(NodeSeed {
                budget: self.budget,
                depth: self.depth + 1,
                method: is_method,
                count_node: true,
            })?;
        }
        Ok(Kind::Object)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.method {
            return Err(de::Error::custom("method-type"));
        }
        let mut count = 0_u64;
        while sequence
            .next_element_seed(NodeSeed {
                budget: self.budget,
                depth: self.depth + 1,
                method: false,
                count_node: true,
            })?
            .is_some()
        {
            count += 1;
            if count > LIMITS.wire.array_entries {
                return Err(de::Error::custom("array-entries"));
            }
        }
        Ok(Kind::Other)
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(value)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if self.method && value.len() as u64 > LIMITS.wire.method_bytes {
            return Err(E::custom("method-bytes"));
        }
        Ok(Kind::Other)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.scalar()
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.scalar()
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.scalar()
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.scalar()
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.scalar()
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.scalar()
    }
}

impl NodeVisitor<'_> {
    fn scalar<E: de::Error>(&self) -> Result<Kind, E> {
        if self.method {
            Err(E::custom("method-type"))
        } else {
            Ok(Kind::Other)
        }
    }
}

fn map_error(error: serde_json::Error) -> StructuralError {
    let text = error.to_string();
    if text.starts_with("depth") {
        StructuralError::Depth
    } else if text.starts_with("nodes") {
        StructuralError::Nodes
    } else if text.starts_with("object-members") {
        StructuralError::ObjectTooLarge
    } else if text.starts_with("array-entries") {
        StructuralError::ArrayTooLarge
    } else if text.starts_with("decoded-key") {
        StructuralError::DecodedKeyTooLarge
    } else if text.starts_with("duplicate-key") {
        StructuralError::DuplicateKey
    } else if text.starts_with("method-bytes") {
        StructuralError::MethodTooLarge
    } else if text.starts_with("method-type") {
        StructuralError::MethodType
    } else {
        StructuralError::Syntax
    }
}
