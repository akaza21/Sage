//! Text rows written before normalized payloads.

use crate::acp::{contract::SafeText, intent::NormalizedPayload, projection::TextRole};

pub(crate) fn parse_legacy_text(
    value: &str,
) -> Result<(NormalizedPayload, Option<TextRole>), serde_json::Error> {
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        role: String,
        text: SafeText<262144>,
        #[serde(default, rename = "attachedContext")]
        _attached_context: crate::acp::attached_context::AttachedContexts,
        /// How many staged resources the prompt carried; never their bytes.
        #[serde(default, rename = "attachments")]
        _attachments: u8,
        #[serde(default, rename = "sessionAction")]
        _session_action: Option<serde::de::IgnoredAny>,
    }
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TaggedWire {
        kind: String,
        payload: Wire,
    }
    let wire = match serde_json::from_str::<Wire>(value) {
        Ok(wire) => wire,
        Err(_) => {
            let tagged: TaggedWire = serde_json::from_str(value)?;
            if tagged.kind != "text" {
                return Err(serde_json::from_str::<Wire>("null").unwrap_err());
            }
            tagged.payload
        }
    };
    let role = match wire.role.as_str() {
        "user" => TextRole::User,
        "assistant" => TextRole::Assistant,
        "system" => TextRole::System,
        _ => return Err(serde_json::from_str::<Wire>("null").unwrap_err()),
    };
    Ok((NormalizedPayload::Text { text: wire.text }, Some(role)))
}
