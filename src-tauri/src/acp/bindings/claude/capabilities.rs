use serde_json::Value;

use crate::acp::{
    contract::{BoundedVec, ContractError, RequiredText, SafeText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{
    sanitize::normalized_text,
    wire::{WireError, decode_frame},
};

const MAX_MODELS: usize = 128;
const MAX_MODEL_BYTES: usize = 262_144;

/// Only capability facts represented by the pinned ACP initialize response.
/// Adapter-private metadata is intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeCapabilityOffer {
    pub protocol_version: String,
    pub resume_session: bool,
    pub load_session: bool,
    pub close_session: bool,
    pub prompt: bool,
    pub cancel: bool,
    pub models: Vec<ClaudeModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeModel {
    pub id: RequiredText<512>,
    pub label: SafeText<512>,
}

impl ClaudeCapabilityOffer {
    pub(crate) fn from_initialize_bytes(raw: &[u8]) -> Result<Self, CapabilityError> {
        let value = decode_frame(raw)?;
        Self::from_initialize_value(&value)
    }

    pub(super) fn from_initialize_value(value: &Value) -> Result<Self, CapabilityError> {
        let result = value
            .get("result")
            .and_then(Value::as_object)
            .ok_or(CapabilityError::MalformedInitialize)?;
        let protocol_version = match result.get("protocolVersion") {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) if value.as_u64() == Some(1) => "1".to_owned(),
            _ => return Err(CapabilityError::MalformedInitialize),
        };
        let capabilities = result
            .get("agentCapabilities")
            .and_then(Value::as_object)
            .ok_or(CapabilityError::MalformedInitialize)?;
        let load_session = capabilities
            .get("loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let session_capabilities = capabilities
            .get("sessionCapabilities")
            .and_then(Value::as_object);
        let resume_session = session_capabilities
            .is_some_and(|value| value.get("resume").is_some_and(Value::is_object));
        let close_session = session_capabilities
            .is_some_and(|value| value.get("close").is_some_and(Value::is_object));

        let mut models = Vec::new();
        let mut model_bytes = 0_usize;
        if let Some(entries) = result.get("models").and_then(Value::as_array) {
            if entries.len() > MAX_MODELS {
                return Err(CapabilityError::ModelsLimit);
            }
            for entry in entries {
                let object = entry.as_object().ok_or(CapabilityError::MalformedModel)?;
                let id = object
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or(CapabilityError::MalformedModel)?;
                let label = object.get("name").and_then(Value::as_str).unwrap_or(id);
                model_bytes = model_bytes
                    .checked_add(id.len())
                    .and_then(|bytes| bytes.checked_add(label.len()))
                    .ok_or(CapabilityError::ModelsLimit)?;
                if model_bytes > MAX_MODEL_BYTES {
                    return Err(CapabilityError::ModelsLimit);
                }
                let safe_id = normalized_text::<512>("modelId", id)?;
                if safe_id.as_str() != id {
                    return Err(CapabilityError::MalformedModel);
                }
                models.push(ClaudeModel {
                    id: RequiredText::new("modelId", id)?,
                    label: normalized_text("modelLabel", label)?,
                });
            }
        }

        Ok(Self {
            protocol_version,
            resume_session,
            load_session,
            close_session,
            // new_session, prompt, and cancel are the admitted ACP core surface.
            // Separate booleans make a future schema downgrade fail closed.
            prompt: true,
            cancel: true,
            models,
        })
    }
}

pub(crate) fn negotiate_capabilities(
    offer: &ClaudeCapabilityOffer,
) -> Result<CapabilitySnapshotV1, CapabilityError> {
    if offer.protocol_version != "1" && offer.protocol_version != "1.0" {
        return Err(CapabilityError::ProtocolVersion);
    }
    let models = BoundedVec::<_, MAX_MODELS>::new("models", offer.models.clone())?;
    let model_count =
        u16::try_from(models.as_slice().len()).map_err(|_| CapabilityError::ModelsLimit)?;
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", &offer.protocol_version)?,
        new_session: true,
        resume_session: offer.resume_session,
        load_session: offer.load_session,
        close_session: offer.close_session,
        prompt: offer.prompt,
        cancel: offer.cancel,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count,
        config_count: 0,
    })
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CapabilityError {
    #[error("Claude initialize response is malformed")]
    MalformedInitialize,
    #[error("Claude model entry is malformed")]
    MalformedModel,
    #[error("Claude model discovery exceeded its count or byte limit")]
    ModelsLimit,
    #[error("Claude ACP protocol version is not admitted")]
    ProtocolVersion,
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Wire(#[from] WireError),
}
