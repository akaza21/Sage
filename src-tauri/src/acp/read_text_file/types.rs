use std::{fmt, ops::Deref};

use serde::{
    Deserialize, Deserializer,
    de::{Error, Visitor},
};

use crate::{
    acp::contract::{
        ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
    },
    domain::ResourceRef,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadTextOwner {
    pub(crate) project_id: SageProjectId,
    pub(crate) project_generation: ProjectGeneration,
    pub(crate) chat_id: SageChatId,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadTextFlushEvent {
    pub(crate) owner: ReadTextOwner,
    pub(crate) resource: ResourceRef,
    /// Durable identity reserved at begin. Active editor identity is bound atomically at acknowledgement.
    pub(crate) expected_editor_revision: Option<u64>,
    pub(crate) token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiskAllowedState {
    NotOpen,
    OpenClean,
}

/// First no-content phase. The renderer declares only the selected editor's
/// identity/state; native binds it to the token before authorizing a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReadTextClassification {
    NotOpen,
    OpenClean,
    Dirty,
    Persisted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ReadTextNextAction {
    DiskAllowed {
        state: DiskAllowedState,
        editor_revision: Option<u64>,
    },
    FlushRequired {
        editor_revision: u64,
        previous_durable_revision: Option<u64>,
    },
    Persisted {
        editor_revision: u64,
    },
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ReadTextFlushState {
    Classify {
        state: ReadTextClassification,
        editor_revision: Option<u64>,
    },
    DiskAllowed {
        state: DiskAllowedState,
        editor_revision: Option<u64>,
    },
    Persisted {
        editor_revision: u64,
        durable_revision: u64,
    },
}
const MAX_P02_TOKEN_BYTES: usize = 512;
const MAX_P02_ROOT_ID_BYTES: usize = 512;
const MAX_P02_RELATIVE_PATH_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedText<const MAX: usize>(String);
impl<const MAX: usize> Deref for BoundedText<MAX> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl<const MAX: usize> fmt::Display for BoundedText<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl<const MAX: usize> From<String> for BoundedText<MAX> {
    fn from(value: String) -> Self {
        assert!(value.len() <= MAX, "test/construction P-02 text is bounded");
        Self(value)
    }
}
impl<const MAX: usize> From<&str> for BoundedText<MAX> {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl<'de, const MAX: usize> Deserialize<'de> for BoundedText<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BoundedVisitor<const MAX: usize>;
        impl<'de, const MAX: usize> Visitor<'de> for BoundedVisitor<MAX> {
            type Value = BoundedText<MAX>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a UTF-8 string no longer than {MAX} bytes")
            }
            fn visit_borrowed_str<E: Error>(self, value: &'de str) -> Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("P-02 string exceeds byte limit"));
                }
                Ok(BoundedText(value.into()))
            }
            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("P-02 string exceeds byte limit"));
                }
                Ok(BoundedText(value.into()))
            }
            fn visit_string<E: Error>(self, value: String) -> Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("P-02 string exceeds byte limit"));
                }
                Ok(BoundedText(value))
            }
        }
        deserializer.deserialize_str(BoundedVisitor::<MAX>)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct P02ResourceRef {
    root_id: BoundedText<MAX_P02_ROOT_ID_BYTES>,
    relative_path: BoundedText<MAX_P02_RELATIVE_PATH_BYTES>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadTextFlushAckWire {
    owner: ReadTextOwner,
    resource: P02ResourceRef,
    expected_editor_revision: Option<u64>,
    token: BoundedText<MAX_P02_TOKEN_BYTES>,
    state: ReadTextFlushState,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadTextFlushAck {
    pub(crate) owner: ReadTextOwner,
    pub(crate) resource: ResourceRef,
    pub(crate) expected_editor_revision: Option<u64>,
    pub(crate) token: BoundedText<MAX_P02_TOKEN_BYTES>,
    pub(crate) state: ReadTextFlushState,
}
impl<'de> Deserialize<'de> for ReadTextFlushAck {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ReadTextFlushAckWire::deserialize(deserializer)?;
        Ok(Self {
            owner: wire.owner,
            resource: ResourceRef {
                root_id: wire.resource.root_id.0,
                relative_path: wire.resource.relative_path.0,
            },
            expected_editor_revision: wire.expected_editor_revision,
            token: wire.token,
            state: wire.state,
        })
    }
}
