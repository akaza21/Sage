use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{AdapterError, CodexAdapter, NegotiatedMethods};
use crate::acp::bindings::codex::request::APP_SERVER_CLIENT_IDENTITY;

const INITIALIZE_RESULT_FIELDS: [&str; 4] =
    ["userAgent", "codexHome", "platformFamily", "platformOs"];
const USER_AGENT_BYTES_MAX: usize = 1024;

impl CodexAdapter {
    pub(crate) fn accept_initialize(
        &mut self,
        result: &Map<String, Value>,
    ) -> Result<CapabilitySnapshotV1, AdapterError> {
        if result.len() != INITIALIZE_RESULT_FIELDS.len()
            || INITIALIZE_RESULT_FIELDS
                .iter()
                .any(|field| !result.contains_key(*field))
        {
            return Err(AdapterError::CapabilityUnavailable);
        }
        let text = |name: &str| {
            result
                .get(name)
                .and_then(Value::as_str)
                .ok_or(AdapterError::CapabilityUnavailable)
        };
        if !valid_user_agent(text("userAgent")?, &self.runtime_version)
            || text("codexHome")? != self.codex_home
            || text("platformFamily")? != "windows"
            || text("platformOs")? != "windows"
        {
            return Err(AdapterError::CapabilityUnavailable);
        }

        let ceiling = self.capability_ceiling.clone();
        let snapshot = CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new(
                "protocolVersion",
                ceiling.protocol_identity.as_str(),
            )?,
            new_session: ceiling.new_session,
            resume_session: ceiling.resume_session,
            load_session: ceiling.load_session,
            close_session: ceiling.close_session,
            prompt: ceiling.prompt,
            cancel: ceiling.cancel,
            native_session_fork: false,
            active_turn_steer: false,
            child_conversation_projection: ceiling.child_conversation_projection,
            child_transcript_read: false,
            collaboration_event_projection: ceiling.collaboration_event_projection,
            multi_root_extension_schema_id: None,
            model_count: 0,
            config_count: 0,
        };
        self.negotiated = Some(NegotiatedMethods {
            snapshot: snapshot.clone(),
            models: ceiling.model_list,
        });
        Ok(snapshot)
    }
}

fn valid_user_agent(value: &str, runtime_version: &str) -> bool {
    if value.is_empty() || value.len() > USER_AGENT_BYTES_MAX || value.chars().any(char::is_control)
    {
        return false;
    }
    let client = APP_SERVER_CLIENT_IDENTITY;
    let prefix = format!("{}/{runtime_version} (Windows ", client.name);
    let suffix = format!(" ({}; {})", client.name, client.version);
    let Some(middle) = value
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
    else {
        return false;
    };
    let Some((windows_version, terminal)) = middle.split_once("; x86_64) ") else {
        return false;
    };
    !windows_version.is_empty()
        && windows_version.len() <= 64
        && windows_version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && !terminal.is_empty()
}
