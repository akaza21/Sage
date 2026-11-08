use serde::{Deserialize, Deserializer, Serialize, de};

use crate::acp::contract::{RequiredText, SchemaVersionV1};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshotV1 {
    pub schema_version: SchemaVersionV1,
    pub protocol_version: RequiredText<64>,
    pub new_session: bool,
    pub resume_session: bool,
    pub load_session: bool,
    pub close_session: bool,
    pub prompt: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub prompt_resources: bool,
    /// ACP `promptCapabilities.image`: the prompt may carry image blocks.
    #[serde(skip_serializing_if = "is_false")]
    pub prompt_images: bool,
    pub cancel: bool,
    pub native_session_fork: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub native_session_compact: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub native_review: bool,
    pub active_turn_steer: bool,
    pub child_conversation_projection: bool,
    pub child_transcript_read: bool,
    pub collaboration_event_projection: bool,
    pub multi_root_extension_schema_id: Option<RequiredText<128>>,
    pub model_count: u16,
    pub config_count: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CapabilitySnapshotWire {
    schema_version: SchemaVersionV1,
    protocol_version: RequiredText<64>,
    new_session: bool,
    resume_session: bool,
    load_session: bool,
    close_session: bool,
    prompt: bool,
    #[serde(default)]
    prompt_resources: bool,
    #[serde(default)]
    prompt_images: bool,
    cancel: bool,
    native_session_fork: bool,
    #[serde(default)]
    native_session_compact: bool,
    #[serde(default)]
    native_review: bool,
    active_turn_steer: bool,
    child_conversation_projection: bool,
    child_transcript_read: bool,
    collaboration_event_projection: bool,
    multi_root_extension_schema_id: Option<RequiredText<128>>,
    model_count: u16,
    config_count: u16,
}

impl<'de> Deserialize<'de> for CapabilitySnapshotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CapabilitySnapshotWire::deserialize(deserializer)?;
        if wire.model_count > 128 || wire.config_count > 128 {
            return Err(de::Error::custom(
                "invalid capability snapshot version or count",
            ));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            protocol_version: wire.protocol_version,
            new_session: wire.new_session,
            resume_session: wire.resume_session,
            load_session: wire.load_session,
            close_session: wire.close_session,
            prompt: wire.prompt,
            prompt_resources: wire.prompt_resources,
            prompt_images: wire.prompt_images,
            cancel: wire.cancel,
            native_session_fork: wire.native_session_fork,
            native_session_compact: wire.native_session_compact,
            native_review: wire.native_review,
            active_turn_steer: wire.active_turn_steer,
            child_conversation_projection: wire.child_conversation_projection,
            child_transcript_read: wire.child_transcript_read,
            collaboration_event_projection: wire.collaboration_event_projection,
            multi_root_extension_schema_id: wire.multi_root_extension_schema_id,
            model_count: wire.model_count,
            config_count: wire.config_count,
        })
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

/// One standard ACP `agentCapabilities.promptCapabilities` flag from an
/// `initialize` response. Only an explicit boolean `true` admits it, so an
/// absent, malformed or downgraded offer fails closed.
pub(crate) fn offers_prompt_capability(initialize: &serde_json::Value, name: &str) -> bool {
    initialize
        .get("result")
        .and_then(|result| result.get("agentCapabilities"))
        .and_then(|capabilities| capabilities.get("promptCapabilities"))
        .and_then(|prompt| prompt.get(name))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}
