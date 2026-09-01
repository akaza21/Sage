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
        let valid = valid_initialize_result(
            result,
            &self.runtime_version,
            &self.codex_home,
            self.codex_home.is_empty(),
        );
        if !valid {
            tracing::warn!(
                runtime_version = self.runtime_version,
                observed = self.codex_home.is_empty(),
                result_fields = ?result.keys().collect::<Vec<_>>(),
                user_agent = ?result.get("userAgent"),
                codex_home = ?result.get("codexHome"),
                platform_family = ?result.get("platformFamily"),
                platform_os = ?result.get("platformOs"),
                "Codex discovery initialize identity validation failed"
            );
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

pub(crate) fn valid_initialize_result(
    result: &Map<String, Value>,
    runtime_version: &str,
    codex_home: &str,
    observed: bool,
) -> bool {
    let text = |name: &str| result.get(name).and_then(Value::as_str);
    let (Some(user_agent), Some(observed_home)) = (text("userAgent"), text("codexHome")) else {
        return false;
    };
    if observed {
        return result.len() == INITIALIZE_RESULT_FIELDS.len()
            && INITIALIZE_RESULT_FIELDS
                .iter()
                .all(|field| result.contains_key(*field))
            && valid_user_agent(user_agent, runtime_version)
            && valid_observed_codex_home(observed_home)
            && valid_platform_identity(
                text("platformFamily").unwrap_or_default(),
                text("platformOs").unwrap_or_default(),
                true,
            );
    }
    result.len() == INITIALIZE_RESULT_FIELDS.len()
        && INITIALIZE_RESULT_FIELDS
            .iter()
            .all(|field| result.contains_key(*field))
        && valid_user_agent(user_agent, runtime_version)
        && observed_home == codex_home
        && valid_platform_identity(
            text("platformFamily").unwrap_or_default(),
            text("platformOs").unwrap_or_default(),
            false,
        )
}

fn valid_observed_codex_home(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32 * 1024
        && !value.chars().any(char::is_control)
        && (value.starts_with('/')
            || value.as_bytes().get(0..3).is_some_and(|prefix| {
                prefix[0].is_ascii_alphabetic()
                    && prefix[1] == b':'
                    && matches!(prefix[2], b'\\' | b'/')
            }))
}

fn valid_platform_identity(family: &str, os: &str, observed: bool) -> bool {
    if !observed {
        return family == "windows" && os == "windows";
    }
    matches!(
        (family, os),
        ("windows", "windows") | ("macos", "macos") | ("macOS", "macOS") | ("unix", "linux")
    )
}

fn valid_user_agent(value: &str, runtime_version: &str) -> bool {
    if value.is_empty() || value.len() > USER_AGENT_BYTES_MAX || value.chars().any(char::is_control)
    {
        return false;
    }
    let client = APP_SERVER_CLIENT_IDENTITY;
    let runtime_version = runtime_version
        .split_ascii_whitespace()
        .last()
        .unwrap_or(runtime_version);
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
