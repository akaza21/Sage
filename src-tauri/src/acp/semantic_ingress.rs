#[cfg(test)]
use serde_json::{Map, Value};

#[cfg(test)]
use crate::acp::intent::CapabilitySnapshotV1;
use crate::acp::{bindings::adapter::AdapterModel, limits::LIMITS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticIngressError {
    #[cfg(test)]
    Invalid,
    Limit,
}

#[cfg(test)]
pub(crate) fn decode_capability_token(
    bytes: &[u8],
) -> Result<CapabilitySnapshotV1, SemanticIngressError> {
    if bytes.len() > LIMITS.wire.capability_metadata_bytes as usize {
        return Err(SemanticIngressError::Limit);
    }
    serde_json::from_slice(bytes).map_err(|_| SemanticIngressError::Invalid)
}

#[cfg(test)]
pub(crate) fn validate_metadata(metadata: &Map<String, Value>) -> Result<(), SemanticIngressError> {
    if metadata.len() > LIMITS.wire.metadata_entries as usize {
        return Err(SemanticIngressError::Limit);
    }
    let bytes = metadata.iter().try_fold(0_usize, |total, (key, value)| {
        let value = value.as_str().ok_or(SemanticIngressError::Invalid)?;
        total
            .checked_add(key.len())
            .and_then(|next| next.checked_add(value.len()))
            .ok_or(SemanticIngressError::Limit)
    })?;
    if bytes > LIMITS.wire.metadata_bytes as usize {
        return Err(SemanticIngressError::Limit);
    }
    Ok(())
}

pub(crate) fn validate_models(values: &[AdapterModel]) -> Result<(), SemanticIngressError> {
    if values.len() > LIMITS.wire.models_or_config_entries as usize {
        return Err(SemanticIngressError::Limit);
    }
    let bytes = values.iter().try_fold(0_usize, |total, model| {
        total
            .checked_add(model.actual_bytes().ok_or(SemanticIngressError::Limit)?)
            .ok_or(SemanticIngressError::Limit)
    })?;
    if bytes > LIMITS.wire.models_or_config_bytes as usize {
        return Err(SemanticIngressError::Limit);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_permission_payload(bytes: &[u8]) -> Result<(), SemanticIngressError> {
    if bytes.len() > LIMITS.wire.permission_payload_bytes as usize {
        return Err(SemanticIngressError::Limit);
    }
    Ok(())
}
