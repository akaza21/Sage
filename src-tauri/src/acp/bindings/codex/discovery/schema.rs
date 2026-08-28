use serde_json::{Map, Value, json};

use crate::acp::{
    contract::RequiredText,
    intent::{SafeResourceRef, UnavailableResourceReason},
    limits::LIMITS,
    transport::{Frame, OutboundFrame},
};

use super::{CodexDiscoveryError, CodexModelSnapshot, DiscoveryRequest};

pub(super) fn request_frame(
    id: u64,
    bytes: Vec<u8>,
) -> Result<DiscoveryRequest, CodexDiscoveryError> {
    Ok(DiscoveryRequest {
        id,
        frame: OutboundFrame::new(bytes).map_err(|_| CodexDiscoveryError::Protocol)?,
    })
}

pub(super) fn json_request(
    id: u64,
    method: &str,
    params: Value,
) -> Result<DiscoveryRequest, CodexDiscoveryError> {
    let bytes = serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))
        .map_err(|_| CodexDiscoveryError::Protocol)?;
    if bytes.len() >= LIMITS.wire.frame_bytes as usize
        || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
    {
        return Err(CodexDiscoveryError::Protocol);
    }
    request_frame(id, bytes)
}

pub(super) fn require_response_id(frame: &Frame, expected: u64) -> Result<(), CodexDiscoveryError> {
    response_result(frame, expected).map(drop)
}

pub(super) fn response_result(
    frame: &Frame,
    expected: u64,
) -> Result<Map<String, Value>, CodexDiscoveryError> {
    let value: Value =
        serde_json::from_slice(frame.as_bytes()).map_err(|_| CodexDiscoveryError::Protocol)?;
    crate::acp::bindings::codex::incoming::validate_shape(&value)
        .map_err(|_| CodexDiscoveryError::Protocol)?;
    let object = value.as_object().ok_or(CodexDiscoveryError::Protocol)?;
    if let Some(error) = object.get("error").and_then(Value::as_object) {
        tracing::warn!(
            expected_response_id = expected,
            json_rpc_error_code = ?error.get("code"),
            json_rpc_error_message = ?error.get("message"),
            "Codex rejected a discovery JSON-RPC request"
        );
    }
    if object.len() != 2
        || object.get("id").and_then(Value::as_u64) != Some(expected)
        || object.contains_key("error")
        || object.contains_key("method")
    {
        tracing::warn!(
            expected_response_id = expected,
            response_id = ?object.get("id"),
            response_fields = ?object.keys().collect::<Vec<_>>(),
            json_rpc_method = ?object.get("method"),
            json_rpc_params = ?object.get("params"),
            "Codex discovery response failed JSON-RPC correlation"
        );
        return Err(CodexDiscoveryError::Protocol);
    }
    object
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .ok_or(CodexDiscoveryError::Protocol)
}

pub(crate) fn managed_chatgpt(account: &Map<String, Value>) -> Result<bool, CodexDiscoveryError> {
    if account.get("type").and_then(Value::as_str) != Some("chatgpt") {
        return Ok(false);
    }
    if account.len() != 3 || !account.contains_key("email") || !account.contains_key("planType") {
        return Err(CodexDiscoveryError::Protocol);
    }
    let email_valid = matches!(account.get("email"), Some(Value::Null))
        || account
            .get("email")
            .and_then(Value::as_str)
            .is_some_and(|email| email.len() <= 8_192 && !email.contains('\0'));
    const PLANS: [&str; 12] = [
        "free",
        "go",
        "plus",
        "pro",
        "prolite",
        "team",
        "self_serve_business_usage_based",
        "business",
        "enterprise_cbp_usage_based",
        "enterprise",
        "edu",
        "unknown",
    ];
    let plan_valid = account
        .get("planType")
        .and_then(Value::as_str)
        .is_some_and(|plan| PLANS.contains(&plan));
    if !email_valid || !plan_valid {
        return Err(CodexDiscoveryError::Protocol);
    }
    Ok(true)
}

pub(super) fn parse_model(value: &Value) -> Result<CodexModelSnapshot, CodexDiscoveryError> {
    let model = value.as_object().ok_or(CodexDiscoveryError::ModelCatalog)?;
    const MODEL_FIELDS: [&str; 16] = [
        "id",
        "model",
        "upgrade",
        "upgradeInfo",
        "availabilityNux",
        "displayName",
        "description",
        "hidden",
        "supportedReasoningEfforts",
        "defaultReasoningEffort",
        "inputModalities",
        "supportsPersonality",
        "additionalSpeedTiers",
        "serviceTiers",
        "defaultServiceTier",
        "isDefault",
    ];
    if model.len() != MODEL_FIELDS.len()
        || !MODEL_FIELDS.iter().all(|field| model.contains_key(*field))
    {
        return Err(CodexDiscoveryError::ModelCatalog);
    }
    let id = model_text(model, "id", 512)?;
    model_text(model, "model", 512)?;
    let name = model_text(model, "displayName", 512)?;
    model_text(model, "description", 8_192)?;
    optional_model_text(model.get("upgrade"), 512)?;
    optional_model_text(model.get("defaultServiceTier"), 512)?;
    validate_upgrade_info(model.get("upgradeInfo"))?;
    validate_availability_nux(model.get("availabilityNux"))?;
    model_text(model, "defaultReasoningEffort", 512)?;
    let efforts = model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .filter(|efforts| efforts.len() <= 32)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    for effort in efforts {
        let effort = effort
            .as_object()
            .ok_or(CodexDiscoveryError::ModelCatalog)?;
        if effort.len() != 2 {
            return Err(CodexDiscoveryError::ModelCatalog);
        }
        model_text(effort, "reasoningEffort", 512)?;
        model_text(effort, "description", 8_192)?;
    }
    validate_input_modalities(model.get("inputModalities"))?;
    bounded_text_array(model.get("additionalSpeedTiers"), 32, 512)?;
    validate_service_tiers(model.get("serviceTiers"))?;
    model
        .get("supportsPersonality")
        .and_then(Value::as_bool)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    let hidden = model
        .get("hidden")
        .and_then(Value::as_bool)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    let is_default = model
        .get("isDefault")
        .and_then(Value::as_bool)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    if hidden {
        return Err(CodexDiscoveryError::ModelCatalog);
    }
    Ok(CodexModelSnapshot {
        id: RequiredText::new("modelId", id.to_owned())
            .map_err(|_| CodexDiscoveryError::ModelCatalog)?,
        name: RequiredText::new("modelName", name.to_owned())
            .map_err(|_| CodexDiscoveryError::ModelCatalog)?,
        is_default,
    })
}

pub(super) fn parse_cursor(
    value: Option<&Value>,
) -> Result<Option<RequiredText<512>>, CodexDiscoveryError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cursor)) => RequiredText::new("modelCursor", cursor.clone())
            .map(Some)
            .map_err(|_| CodexDiscoveryError::ModelCatalog),
        Some(_) => Err(CodexDiscoveryError::ModelCatalog),
    }
}

fn optional_model_text(value: Option<&Value>, maximum: usize) -> Result<(), CodexDiscoveryError> {
    match value {
        Some(Value::Null) => Ok(()),
        Some(Value::String(value)) if value.len() <= maximum && !value.contains('\0') => Ok(()),
        _ => Err(CodexDiscoveryError::ModelCatalog),
    }
}

fn bounded_text_array(
    value: Option<&Value>,
    entries: usize,
    bytes_each: usize,
) -> Result<(), CodexDiscoveryError> {
    let values = value
        .and_then(Value::as_array)
        .filter(|values| values.len() <= entries)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    if values.iter().all(|value| {
        value.as_str().is_some_and(|value| {
            !value.is_empty() && value.len() <= bytes_each && !value.contains('\0')
        })
    }) {
        Ok(())
    } else {
        Err(CodexDiscoveryError::ModelCatalog)
    }
}

fn validate_input_modalities(value: Option<&Value>) -> Result<(), CodexDiscoveryError> {
    let modalities = value
        .and_then(Value::as_array)
        .filter(|modalities| modalities.len() <= 8)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    if modalities.iter().all(|modality| {
        modality
            .as_str()
            .is_some_and(|modality| matches!(modality, "text" | "image" | "audio"))
    }) {
        Ok(())
    } else {
        Err(CodexDiscoveryError::ModelCatalog)
    }
}

fn validate_upgrade_info(value: Option<&Value>) -> Result<(), CodexDiscoveryError> {
    let Some(value) = value else {
        return Err(CodexDiscoveryError::ModelCatalog);
    };
    if value.is_null() {
        return Ok(());
    }
    let object = value.as_object().ok_or(CodexDiscoveryError::ModelCatalog)?;
    if object.len() != 4
        || !["model", "upgradeCopy", "modelLink", "migrationMarkdown"]
            .iter()
            .all(|field| object.contains_key(*field))
    {
        return Err(CodexDiscoveryError::ModelCatalog);
    }
    model_text(object, "model", 512)?;
    for field in ["upgradeCopy", "modelLink", "migrationMarkdown"] {
        optional_model_text(object.get(field), 8_192)?;
    }
    Ok(())
}

fn validate_availability_nux(value: Option<&Value>) -> Result<(), CodexDiscoveryError> {
    let Some(value) = value else {
        return Err(CodexDiscoveryError::ModelCatalog);
    };
    if value.is_null() {
        return Ok(());
    }
    let object = value.as_object().ok_or(CodexDiscoveryError::ModelCatalog)?;
    if object.len() != 1 {
        return Err(CodexDiscoveryError::ModelCatalog);
    }
    model_text(object, "message", 8_192).map(drop)
}

fn validate_service_tiers(value: Option<&Value>) -> Result<(), CodexDiscoveryError> {
    let tiers = value
        .and_then(Value::as_array)
        .filter(|tiers| tiers.len() <= 32)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    for tier in tiers {
        let tier = tier.as_object().ok_or(CodexDiscoveryError::ModelCatalog)?;
        if tier.len() != 3
            || !["id", "name", "description"]
                .iter()
                .all(|field| tier.contains_key(*field))
        {
            return Err(CodexDiscoveryError::ModelCatalog);
        }
        model_text(tier, "id", 512)?;
        model_text(tier, "name", 512)?;
        model_text(tier, "description", 8_192)?;
    }
    Ok(())
}

fn model_text<'a>(
    model: &'a Map<String, Value>,
    key: &str,
    maximum: usize,
) -> Result<&'a str, CodexDiscoveryError> {
    let value = model
        .get(key)
        .and_then(Value::as_str)
        .ok_or(CodexDiscoveryError::ModelCatalog)?;
    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        Err(CodexDiscoveryError::ModelCatalog)
    } else {
        Ok(value)
    }
}

pub(super) struct NoResources;

impl crate::acp::bindings::codex::incoming::ResourceMapper for NoResources {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}
