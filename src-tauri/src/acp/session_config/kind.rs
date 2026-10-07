use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Choices, ConfigId, SafeText};

const UNSUPPORTED: &str = "unsupported";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigKind {
    Select {
        current_value: ConfigId,
        options: Choices,
    },
    Boolean {
        current_value: bool,
    },
    Unsupported {
        kind: SafeText<1024>,
    },
}

impl ConfigKind {
    fn from_raw(raw: serde_json::Map<String, Value>) -> Result<Self, String> {
        match raw.get("type").and_then(Value::as_str) {
            Some("select") => {
                let current_value = ConfigId::new(
                    "currentValue",
                    raw.get("currentValue")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
                .map_err(|error| error.to_string())?;
                let options =
                    Choices::deserialize(raw.get("options").cloned().unwrap_or(Value::Null))
                        .map_err(|error| error.to_string())?;
                Ok(Self::Select {
                    current_value,
                    options,
                })
            }
            Some("boolean") => match raw.get("currentValue") {
                Some(Value::Bool(current_value)) => Ok(Self::Boolean {
                    current_value: *current_value,
                }),
                _ => Err("expected boolean currentValue for boolean config".into()),
            },
            Some(UNSUPPORTED) => Ok(Self::Unsupported {
                kind: SafeText::new(
                    "kind",
                    raw.get("kind").and_then(Value::as_str).unwrap_or_default(),
                )
                .map_err(|error| error.to_string())?,
            }),
            other => Ok(Self::Unsupported {
                kind: SafeText::new("type", other.unwrap_or_default())
                    .map_err(|error| error.to_string())?,
            }),
        }
    }

    pub(super) fn demoted(&self) -> Result<Self, String> {
        match self {
            Self::Boolean { .. } => Ok(Self::Unsupported {
                kind: SafeText::new("kind", "boolean").map_err(|error| error.to_string())?,
            }),
            other => Ok(other.clone()),
        }
    }
}

impl Serialize for ConfigKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Select {
                current_value,
                options,
            } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("type", "select")?;
                map.serialize_entry("currentValue", current_value)?;
                map.serialize_entry("options", options)?;
                map.end()
            }
            Self::Boolean { current_value } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "boolean")?;
                map.serialize_entry("currentValue", current_value)?;
                map.end()
            }
            Self::Unsupported { kind } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", UNSUPPORTED)?;
                map.serialize_entry("kind", kind)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ConfigKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KindVisitor;
        impl<'de> serde::de::Visitor<'de> for KindVisitor {
            type Value = ConfigKind;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a session configuration option body")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<ConfigKind, A::Error> {
                let mut raw = serde_json::Map::new();
                while let Some(key) = access.next_key::<String>()? {
                    raw.insert(key, access.next_value()?);
                }
                ConfigKind::from_raw(raw).map_err(serde::de::Error::custom)
            }
        }
        deserializer.deserialize_map(KindVisitor)
    }
}
