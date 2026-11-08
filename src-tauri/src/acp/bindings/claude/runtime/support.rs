use serde::Serialize;
use serde_json::Value;

use crate::acp::{
    bindings::adapter::{
        AdapterEgress, AdapterIdentity, AdapterRuntimeError, EgressToken,
        ResourceMapper as SharedResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{ExternalId, NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    transport::OutboundFrame,
};

use super::super::{
    ResourceMapper,
    capabilities::{CapabilityError, ClaudeModel},
    sanitize::normalized_text,
    translator::TranslationError,
};

pub(super) struct MapperBridge<'a>(pub(super) &'a dyn SharedResourceMapper);

impl ResourceMapper for MapperBridge<'_> {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.0.map_existing(absolute_path)
    }
}

/// `prompt` marks a `session/prompt` command, whose frame may carry image
/// blocks past the ordinary frame bound and rides the egress prompt reserve.
pub(super) fn request(
    id: u64,
    method: &'static str,
    params: impl Serialize,
    prompt: bool,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))
    .map_err(|_| AdapterRuntimeError::Frame)?;
    let frame = if prompt {
        OutboundFrame::new_prompt(bytes)
    } else {
        OutboundFrame::new(bytes)
    }
    .map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(frame, EgressToken::request(id)))
}

pub(super) fn cancel_notification(
    method: &'static str,
    params: impl Serialize,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    }))
    .map_err(|_| AdapterRuntimeError::Frame)?;
    let frame = OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(
        frame,
        EgressToken::cancel_notification(),
    ))
}

pub(super) fn denial(
    request_id: &ExternalId,
    wire_id: &Value,
    replay: bool,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": wire_id,
        "result": { "outcome": { "outcome": "cancelled" } },
    }))
    .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
    let frame =
        OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
    Ok(AdapterEgress::new(
        frame,
        EgressToken::mandatory_deny(request_id.clone(), replay),
    ))
}

pub(super) fn cancelled_permission(
    request_id: &crate::acp::contract::SageServerRequestId,
    wire_id: &Value,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": wire_id,
        "result": { "outcome": { "outcome": "cancelled" } },
    }))
    .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
    let frame =
        OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
    let external = ExternalId::new("permissionRequest", request_id.as_str())
        .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
    Ok(AdapterEgress::new(
        frame,
        EgressToken::permission_cancel(external),
    ))
}

pub(super) fn permission_identity(
    value: Option<&Value>,
) -> Result<(ExternalId, String, Value), AdapterRuntimeError> {
    let (kind, external, wire) = match value {
        Some(Value::String(value)) => ("string", value.clone(), Value::String(value.clone())),
        Some(Value::Number(value)) => {
            let integer = value
                .as_i64()
                .map(|value| value.to_string())
                .or_else(|| {
                    value
                        .as_u64()
                        .filter(|value| *value <= i64::MAX as u64)
                        .map(|value| value.to_string())
                })
                .ok_or(AdapterRuntimeError::Frame)?;
            ("number", integer, Value::Number(value.clone()))
        }
        _ => return Err(AdapterRuntimeError::Frame),
    };
    let token = ExternalId::new("permissionIntentId", format!("{kind}:{external}"))
        .map_err(|_| AdapterRuntimeError::Frame)?;
    Ok((token, external, wire))
}

pub(super) fn response_id(value: &Value) -> Result<u64, AdapterRuntimeError> {
    match value.get("id") {
        Some(Value::Number(value)) => value.as_u64().ok_or(AdapterRuntimeError::Frame),
        Some(Value::String(value)) => value
            .parse::<u64>()
            .ok()
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Frame),
        _ => Err(AdapterRuntimeError::Frame),
    }
}

pub(super) fn result_session_id(value: &Value) -> Result<ExternalId, AdapterRuntimeError> {
    value
        .get("result")
        .and_then(|result| result.get("sessionId"))
        .and_then(Value::as_str)
        .ok_or(AdapterRuntimeError::Frame)
        .and_then(|id| {
            ExternalId::new("externalSessionId", id).map_err(|_| AdapterRuntimeError::Frame)
        })
}

pub(super) fn identity(
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: &SageAgentSessionId,
    external_id: ExternalId,
    turn_id: Option<ExternalId>,
    turn_client_message_id: Option<RequiredText<512>>,
) -> AdapterIdentity {
    AdapterIdentity {
        binding_digest,
        runtime_epoch,
        session_id: session_id.clone(),
        external_id,
        turn_id,
        turn_client_message_id,
    }
}

pub(super) fn model_bytes(models: &[ClaudeModel]) -> Result<usize, AdapterRuntimeError> {
    models
        .iter()
        .try_fold(0_usize, |total, model| {
            total
                .checked_add(model.id.as_str().len())?
                .checked_add(model.label.as_str().len())
        })
        .ok_or(AdapterRuntimeError::BatchBytes)
}

pub(super) fn unsupported(kind: &str) -> Result<NormalizedPayload, AdapterRuntimeError> {
    Ok(NormalizedPayload::Unsupported {
        external_kind: normalized_text("externalKind", kind)
            .map_err(|_| AdapterRuntimeError::Frame)?,
        summary: normalized_text(
            "unsupportedSummary",
            "Unsupported bounded Claude ACP message",
        )
        .map_err(|_| AdapterRuntimeError::Frame)?,
    })
}

pub(super) fn map_translation(error: TranslationError) -> AdapterRuntimeError {
    match error {
        TranslationError::EventLimit
        | TranslationError::ToolLimit
        | TranslationError::ItemLimit
        | TranslationError::PermissionLimit => AdapterRuntimeError::BatchCount,
        TranslationError::PlanLimit => AdapterRuntimeError::BatchBytes,
        _ => AdapterRuntimeError::Frame,
    }
}

pub(super) fn map_capability(_error: CapabilityError) -> AdapterRuntimeError {
    AdapterRuntimeError::Frame
}
