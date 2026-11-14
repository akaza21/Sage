//! A Codex `mcpServerStatus/list` result as `/mcp` reads it. Codex sends
//! every tool's schema whole, so a server with many tools can outgrow the
//! structural bounds the rest of the wire keeps; this reads the page in one
//! pass that counts tools and resources without keeping them, and treats a
//! field of an unexpected type as absent rather than failing the page.

use std::{fmt, marker::PhantomData};

use serde::{
    Deserialize, Deserializer,
    de::{Error, IgnoredAny, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexMcpPage {
    pub(super) data: Vec<Object<CodexServerStatus>>,
    #[serde(default)]
    next_cursor: Option<IgnoredAny>,
}

impl CodexMcpPage {
    pub(super) fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct CodexServerStatus {
    pub(super) name: Text,
    pub(super) server_info: Object<CodexServerInfo>,
    pub(super) runtime_status: Text,
    pub(super) auth_status: Text,
    pub(super) tools: Entries,
    pub(super) resources: Entries,
    pub(super) tools_error: Text,
    pub(super) http_origin: Text,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CodexServerInfo {
    pub(super) title: Text,
    pub(super) version: Text,
}

#[derive(Debug, Default)]
pub(super) struct Text(pub(super) Option<String>);

/// An object read as `T`, or nothing when the field holds anything else.
/// `T` reads any object, so a malformed one never fails the page.
#[derive(Debug)]
pub(super) struct Object<T>(pub(super) Option<T>);

impl<T> Default for Object<T> {
    fn default() -> Self {
        Self(None)
    }
}

/// How many entries a map or list holds; zero for anything else.
#[derive(Debug, Default)]
pub(super) struct Entries(pub(super) u32);

macro_rules! scalars_read_as {
    ($absent:expr) => {
        fn visit_bool<E: Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok($absent)
        }
        fn visit_i64<E: Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok($absent)
        }
        fn visit_u64<E: Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok($absent)
        }
        fn visit_f64<E: Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok($absent)
        }
        fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
            Ok($absent)
        }
    };
}

fn drain_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<usize, A::Error> {
    let mut count = 0;
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {
        count += 1;
    }
    Ok(count)
}

fn drain_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<usize, A::Error> {
    let mut count = 0;
    while seq.next_element::<IgnoredAny>()?.is_some() {
        count += 1;
    }
    Ok(count)
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TextVisitor;
        impl<'de> Visitor<'de> for TextVisitor {
            type Value = Text;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_str<E: Error>(self, value: &str) -> Result<Text, E> {
                Ok(Text(Some(value.to_owned())))
            }

            fn visit_string<E: Error>(self, value: String) -> Result<Text, E> {
                Ok(Text(Some(value)))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Text, A::Error> {
                drain_map(map).map(|_| Text(None))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Text, A::Error> {
                drain_seq(seq).map(|_| Text(None))
            }

            scalars_read_as!(Text(None));
        }
        deserializer.deserialize_any(TextVisitor)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
            type Value = Object<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Object<T>, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(|value| Object(Some(value)))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Object<T>, A::Error> {
                drain_seq(seq).map(|_| Object(None))
            }

            fn visit_str<E: Error>(self, _: &str) -> Result<Object<T>, E> {
                Ok(Object(None))
            }

            scalars_read_as!(Object(None));
        }
        deserializer.deserialize_any(ObjectVisitor(PhantomData))
    }
}

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;
        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = usize;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<usize, A::Error> {
                drain_map(map)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<usize, A::Error> {
                drain_seq(seq)
            }

            fn visit_str<E: Error>(self, _: &str) -> Result<usize, E> {
                Ok(0)
            }

            scalars_read_as!(0);
        }
        let count = deserializer.deserialize_any(EntriesVisitor)?;
        Ok(Self(u32::try_from(count).unwrap_or(u32::MAX)))
    }
}
