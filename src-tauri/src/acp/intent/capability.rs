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
    /// The harness lists sessions of the project started outside Sage and
    /// resumes one of them in a new Chat.
    #[serde(skip_serializing_if = "is_false")]
    pub native_session_import: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub native_review: bool,
    /// The harness lists and stops the commands it left running.
    #[serde(skip_serializing_if = "is_false")]
    pub native_background_terminals: bool,
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

impl CapabilitySnapshotV1 {
    /// Whether a session negotiated with `self` resumes under `negotiated`.
    ///
    /// Fixed facts must match exactly. Each driven capability is either
    /// fixed or an upgrade: a capability Sage learned to drive after a
    /// session was stored. A stored session negotiated it off (or lacks
    /// the field, which reads as off), so the same harness may now
    /// negotiate it on and the session still resumes. Its stored snapshot
    /// stays immutable, so the chat keeps offering the upgrade as off
    /// (fail closed); the live runtime admits only what it negotiated now.
    /// An upgrade never goes back from on to off.
    pub(crate) fn resumes_as(&self, negotiated: &Self) -> bool {
        let (fixed, upgrades) = self.resume_terms();
        let (now_fixed, now_upgrades) = negotiated.resume_terms();
        fixed == now_fixed
            && upgrades
                .iter()
                .zip(now_upgrades)
                .all(|(&was, now)| !was || now)
    }

    /// Splits the snapshot into the facts a resume must match and the
    /// capabilities it may gain. The destructure names every field, so a
    /// new capability cannot compile until it is classified here.
    fn resume_terms(&self) -> (ResumeFixed<'_>, [bool; RESUME_UPGRADES]) {
        let Self {
            schema_version,
            protocol_version,
            new_session,
            resume_session,
            load_session,
            close_session,
            prompt,
            prompt_resources,
            prompt_images,
            cancel,
            native_session_fork,
            native_session_compact,
            native_session_import,
            native_review,
            native_background_terminals,
            active_turn_steer,
            child_conversation_projection,
            child_transcript_read,
            collaboration_event_projection,
            multi_root_extension_schema_id,
            model_count,
            config_count,
        } = self;
        (
            ResumeFixed {
                schema_version,
                protocol_version,
                new_session,
                resume_session,
                load_session,
                close_session,
                prompt,
                cancel,
                child_conversation_projection,
                child_transcript_read,
                collaboration_event_projection,
                multi_root_extension_schema_id,
                model_count,
                config_count,
            },
            [
                *prompt_resources,
                *prompt_images,
                *native_session_fork,
                *native_session_compact,
                *native_session_import,
                *native_review,
                *native_background_terminals,
                *active_turn_steer,
            ],
        )
    }
}

/// How many capabilities [`CapabilitySnapshotV1::resume_terms`] admits as
/// upgrades.
const RESUME_UPGRADES: usize = 8;

/// The facts of a negotiation a resumed session must still match.
#[derive(PartialEq, Eq)]
struct ResumeFixed<'a> {
    schema_version: &'a SchemaVersionV1,
    protocol_version: &'a RequiredText<64>,
    new_session: &'a bool,
    resume_session: &'a bool,
    load_session: &'a bool,
    close_session: &'a bool,
    prompt: &'a bool,
    cancel: &'a bool,
    child_conversation_projection: &'a bool,
    child_transcript_read: &'a bool,
    collaboration_event_projection: &'a bool,
    multi_root_extension_schema_id: &'a Option<RequiredText<128>>,
    model_count: &'a u16,
    config_count: &'a u16,
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
