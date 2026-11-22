use serde::{Deserialize, Deserializer, Serialize, de};

use crate::contract::{RequiredText, SchemaVersionV1};

/// Native goal operations admitted for this session. Missing operations fail closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalCapabilities {
    /// Goal state is reported by the harness, independently of command support.
    #[serde(default)]
    pub observes: bool,
    pub set: bool,
    pub pause: bool,
    pub resume: bool,
    pub token_budget: bool,
    pub clear: bool,
}

impl GoalCapabilities {
    pub fn supported(&self) -> bool {
        self.observes || self.set || self.pause || self.resume || self.token_budget || self.clear
    }

    pub fn intersect(self, other: Self) -> Self {
        Self {
            observes: self.observes && other.observes,
            set: self.set && other.set,
            pause: self.pause && other.pause,
            resume: self.resume && other.resume,
            token_budget: self.token_budget && other.token_budget,
            clear: self.clear && other.clear,
        }
    }
}

fn no_goals(value: &GoalCapabilities) -> bool {
    !value.supported()
}

/// What one harness-process negotiation admits. Activation replaces this
/// snapshot on resume; it is not part of the native session's identity.
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
    /// The harness lists sessions of the project started outside Sage and
    /// resumes one of them in a new Chat.
    #[serde(skip_serializing_if = "is_false")]
    pub native_session_import: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub native_review: bool,
    /// The harness lists and stops the commands it left running.
    #[serde(skip_serializing_if = "is_false")]
    pub native_background_terminals: bool,
    #[serde(skip_serializing_if = "no_goals")]
    pub goal: GoalCapabilities,
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
    native_session_import: bool,
    #[serde(default)]
    native_review: bool,
    #[serde(default)]
    native_background_terminals: bool,
    #[serde(default)]
    goal: GoalCapabilities,
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
            native_session_import: wire.native_session_import,
            native_review: wire.native_review,
            native_background_terminals: wire.native_background_terminals,
            goal: wire.goal,
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
pub fn offers_prompt_capability(initialize: &serde_json::Value, name: &str) -> bool {
    initialize
        .get("result")
        .and_then(|result| result.get("agentCapabilities"))
        .and_then(|capabilities| capabilities.get("promptCapabilities"))
        .and_then(|prompt| prompt.get(name))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}
