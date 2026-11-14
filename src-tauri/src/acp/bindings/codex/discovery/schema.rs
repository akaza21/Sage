use serde_json::{Map, Value, json};

use crate::acp::transport::{Frame, OutboundFrame};
use sage_acp_protocol::{
    contract::RequiredText,
    intent::{SafeResourceRef, UnavailableResourceReason},
};
use sage_core::limits::LIMITS;

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
    // Envelope members other than `id`, `result`, `error` and `method` (such as
    // a `jsonrpc` tag) carry nothing Sage reads and are ignored.
    if object.get("id").and_then(Value::as_u64) != Some(expected)
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

/// An OpenAI API key Codex stores in its own home: `{"type":"apiKey"}`.
pub(crate) fn api_key_account(account: &Map<String, Value>) -> bool {
    account_type(account) == Some("apiKey")
}

/// Other members, such as workspace routing, are not used and may change
/// between releases.
pub(crate) fn valid_account_read(result: &Map<String, Value>) -> bool {
    matches!(result.get("account"), Some(Value::Null | Value::Object(_)))
        && result
            .get("requiresOpenaiAuth")
            .is_some_and(Value::is_boolean)
}

/// Sage keeps no identity data from a ChatGPT sign-in, so the email, plan and
/// workspace members are not read: a new plan or member must not sign the user
/// out of Sage.
pub(crate) fn managed_chatgpt(account: &Map<String, Value>) -> bool {
    account_type(account) == Some("chatgpt")
}

fn account_type(account: &Map<String, Value>) -> Option<&str> {
    account.get("type").and_then(Value::as_str)
}

/// Members Sage does not read (tiers, modalities, upgrade copy and so on) are
/// ignored, so a model the service describes with new metadata is still offered.
/// Hidden models are skipped, as Codex's own picker does. An unreadable
/// thinking level list leaves the model without levels.
pub(super) fn parse_model(
    value: &Value,
) -> Result<Option<CodexModelSnapshot>, CodexDiscoveryError> {
    let model = value.as_object().ok_or(CodexDiscoveryError::ModelCatalog)?;
    // As in the runtime catalog, a flag that is absent or not a boolean is off.
    let flag = |key: &str| model.get(key).and_then(Value::as_bool).unwrap_or(false);
    if flag("hidden") {
        return Ok(None);
    }
    let is_default = flag("isDefault");
    let id = model_text(model, "id", 512)?;
    let name = model_text(model, "displayName", 512)?;
    Ok(Some(CodexModelSnapshot {
        id: RequiredText::new("modelId", id.to_owned())
            .map_err(|_| CodexDiscoveryError::ModelCatalog)?,
        name: RequiredText::new("modelName", name.to_owned())
            .map_err(|_| CodexDiscoveryError::ModelCatalog)?,
        is_default,
        effort_levels: model
            .get("supportedReasoningEfforts")
            .and_then(|efforts| {
                crate::acp::bindings::codex::incoming::parse_reasoning_efforts(efforts).ok()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|effort| effort.id)
            .collect(),
    }))
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
