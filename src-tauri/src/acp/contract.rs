use std::{fmt, marker::PhantomData};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use uuid::Uuid;

use super::limits::LIMITS;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    #[error("{field} must contain between {minimum} and {maximum} UTF-8 bytes")]
    InvalidLength {
        field: &'static str,
        minimum: usize,
        maximum: usize,
    },
    #[error("{field} contains forbidden content")]
    ForbiddenContent { field: &'static str },
    #[error("{field} must be a canonical non-negative decimal within SQLite INTEGER range")]
    InvalidCounter { field: &'static str },
    #[error("{field} exhausted its monotonic SQLite INTEGER range")]
    CounterExhausted { field: &'static str },
    #[error("{field} contains {actual} entries; maximum is {maximum}")]
    TooManyEntries {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct SafeText<const MAX: usize>(String);

impl<const MAX: usize> SafeText<MAX> {
    pub fn new(field: &'static str, value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.len() > MAX {
            return Err(ContractError::InvalidLength {
                field,
                minimum: 0,
                maximum: MAX,
            });
        }
        if value.contains('\0') {
            return Err(ContractError::ForbiddenContent { field });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for SafeText<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new("text", value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RequiredText<const MAX: usize>(SafeText<MAX>);

impl<const MAX: usize> RequiredText<MAX> {
    pub fn new(field: &'static str, value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ContractError::InvalidLength {
                field,
                minimum: 1,
                maximum: MAX,
            });
        }
        Ok(Self(SafeText::new(field, value)?))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for RequiredText<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new("text", String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaVersionV1;

impl Serialize for SchemaVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(1)
    }
}

impl<'de> Deserialize<'de> for SchemaVersionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom("schemaVersion must equal 1")),
        }
    }
}

macro_rules! sage_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            pub fn new() -> Self {
                Self(format!(concat!($prefix, "-{}"), Uuid::new_v4()))
            }

            pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
                let value = value.into();
                if value.is_empty() || value.len() > LIMITS.wire.external_id_bytes as usize {
                    return Err(ContractError::InvalidLength {
                        field: stringify!($name),
                        minimum: 1,
                        maximum: LIMITS.wire.external_id_bytes as usize,
                    });
                }
                if value.contains('\0') {
                    return Err(ContractError::ForbiddenContent {
                        field: stringify!($name),
                    });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

sage_id!(SageProjectId, "project");
sage_id!(SageRootId, "root");
sage_id!(SageChatId, "chat");
sage_id!(SageAgentSessionId, "session");
sage_id!(SageTurnId, "turn");
sage_id!(SageItemId, "item");
sage_id!(SageActivityId, "activity");
sage_id!(SageServerRequestId, "request");
sage_id!(ClientIntentId, "intent");

macro_rules! counter {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            pub const ZERO: Self = Self(0);

            pub fn new(value: u64) -> Result<Self, ContractError> {
                (value <= i64::MAX as u64).then_some(Self(value)).ok_or(
                    ContractError::InvalidCounter {
                        field: stringify!($name),
                    },
                )
            }

            pub fn get(self) -> u64 {
                self.0
            }

            pub fn next(self) -> Result<Self, ContractError> {
                self.0
                    .checked_add(1)
                    .filter(|value| *value <= i64::MAX as u64)
                    .map(Self)
                    .ok_or(ContractError::CounterExhausted {
                        field: stringify!($name),
                    })
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                if raw.is_empty()
                    || (raw.len() > 1 && raw.starts_with('0'))
                    || !raw.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(de::Error::custom(ContractError::InvalidCounter {
                        field: stringify!($name),
                    }));
                }
                let value = raw.parse().map_err(de::Error::custom)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

counter!(ProjectGeneration);
counter!(RuntimeEpoch);
counter!(ProjectionRevision);

/// Whether an intent built on the `expected` view of a chat is still
/// current. `intent` is the revision of the chat's latest send-relevant
/// change and `current` its projection revision: the view may trail only
/// session-neutral changes, and may never be ahead of the chat.
pub fn admits_projection(
    intent: ProjectionRevision,
    current: ProjectionRevision,
    expected: ProjectionRevision,
) -> bool {
    (intent..=current).contains(&expected)
}
counter!(EntityRevision);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BoundedVec<T, const MAX: usize>(Vec<T>);

impl<T, const MAX: usize> Default for BoundedVec<T, MAX> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T, const MAX: usize> BoundedVec<T, MAX> {
    pub fn new(field: &'static str, values: Vec<T>) -> Result<Self, ContractError> {
        if values.len() > MAX {
            return Err(ContractError::TooManyEntries {
                field,
                actual: values.len(),
                maximum: MAX,
            });
        }
        Ok(Self(values))
    }

    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// The entries, to change in place; the bound holds since none can be
    /// added.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<'de, T: Deserialize<'de>, const MAX: usize> Deserialize<'de> for BoundedVec<T, MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor<T, const MAX: usize>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const MAX: usize> de::Visitor<'de> for Visitor<T, MAX> {
            type Value = BoundedVec<T, MAX>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "at most {MAX} entries")
            }
            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
                while let Some(value) = sequence.next_element()? {
                    if values.len() == MAX {
                        return Err(de::Error::custom("bounded list limit exceeded"));
                    }
                    values.push(value);
                }
                Ok(BoundedVec(values))
            }
        }
        deserializer.deserialize_seq(Visitor::<T, MAX>(PhantomData))
    }
}

pub use super::lifecycle::*;
