use std::hash::{Hash, Hasher};

use serde_json::{Map, Value, json};

use crate::acp::{
    contract::SafeText,
    intent::{NormalizedPayload, ToolCategory},
    limits::LIMITS,
    runtime::host_browse::{CURATED_BROWSE_TOOL_NAME, admit_browse_url},
};

use super::super::CODEX_BROWSE_TOOL_NAME;
use super::super::state::{CodexAdapter, ExternalKind, PermissionRegistration};
use super::{
    DenyResponse, IncomingEvent, IngressDisposition, IngressError,
    support::{content, identity, permission, request_id_text, text},
};

pub(super) fn server_request(
    adapter: &mut CodexAdapter,
    object: &Map<String, Value>,
    method: &str,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    if method == "item/tool/call" {
        return dynamic_tool_call(adapter, object, params);
    }
    let request_id = request_id_text(object.get("id").ok_or(IngressError::Malformed)?)?;
    let thread = text(params, "threadId", 512)?;
    let turn = text(params, "turnId", 512)?;
    let item = text(params, "itemId", 512)?;
    adapter.expect_thread(thread).map_err(identity)?;
    adapter.expect_turn(turn).map_err(identity)?;
    let category = match method {
        "item/commandExecution/requestApproval" => ToolCategory::Command,
        "item/fileChange/requestApproval" => ToolCategory::File,
        "item/permissions/requestApproval" => ToolCategory::Other,
        _ => return Err(IngressError::UnsupportedServerRequest),
    };
    let result = if method == "item/permissions/requestApproval" {
        json!({"permissions": {}})
    } else {
        json!({"decision": "decline"})
    };
    let bytes = serde_json::to_vec(&json!({"id": object["id"].clone(), "result": result}))
        .map_err(|_| IngressError::Malformed)?;
    if bytes.len() >= LIMITS.wire.frame_bytes as usize
        || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
    {
        return Err(IngressError::FrameBytes);
    }
    let external_request = match adapter
        .register_permission_request(
            &request_id,
            request_fingerprint(object),
            method,
            turn,
            item,
            &bytes,
        )
        .map_err(permission)?
    {
        PermissionRegistration::New(external) => external,
        PermissionRegistration::DuplicatePending => {
            return Ok(IngressDisposition::DuplicateServerRequest { denial: None });
        }
        PermissionRegistration::DuplicateCommitted(bytes) => {
            return Ok(IngressDisposition::DuplicateServerRequest {
                denial: Some(bytes),
            });
        }
    };
    let denial = DenyResponse {
        external_request,
        bytes,
    };
    let event = IncomingEvent::Permission {
        turn_id: adapter
            .scoped_id(ExternalKind::Turn, turn)
            .map_err(identity)?,
        item_id: adapter
            .scoped_in_turn(ExternalKind::Item, turn, item)
            .map_err(identity)?,
        payload: NormalizedPayload::PermissionRequest {
            category,
            summary: SafeText::new(
                "permissionSummary",
                "Codex requested additional authority; awaiting your decision",
            )
            .map_err(content)?,
        },
        denial: Box::new(denial),
    };
    Ok(event.into())
}

fn dynamic_tool_call(
    adapter: &CodexAdapter,
    object: &Map<String, Value>,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let thread = text(params, "threadId", 512)?;
    let turn = text(params, "turnId", 512)?;
    adapter.expect_thread(thread).map_err(identity)?;
    adapter.expect_turn(turn).map_err(identity)?;
    let _call_id = text(params, "callId", 512)?;
    let tool = text(params, "tool", 128)?;
    if tool != CURATED_BROWSE_TOOL_NAME && tool != CODEX_BROWSE_TOOL_NAME {
        return Err(IngressError::UnsupportedServerRequest);
    }
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let href = args
        .get("url")
        .and_then(Value::as_str)
        .and_then(admit_browse_url)
        .ok_or(IngressError::Malformed)?;
    let bytes = serde_json::to_vec(&json!({
        "id": object["id"].clone(),
        "result": {
            "success": true,
            "contentItems": [{"type": "inputText", "text": href}]
        }
    }))
    .map_err(|_| IngressError::Malformed)?;
    if bytes.len() >= LIMITS.wire.frame_bytes as usize
        || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
    {
        return Err(IngressError::FrameBytes);
    }
    Ok(IncomingEvent::HostToolReply { bytes }.into())
}

fn request_fingerprint(object: &Map<String, Value>) -> u64 {
    fn hash_value(value: &Value, hasher: &mut impl Hasher) {
        std::mem::discriminant(value).hash(hasher);
        match value {
            Value::Null => {}
            Value::Bool(value) => value.hash(hasher),
            Value::Number(value) => value.to_string().hash(hasher),
            Value::String(value) => value.hash(hasher),
            Value::Array(values) => values.iter().for_each(|value| hash_value(value, hasher)),
            Value::Object(values) => values.iter().for_each(|(key, value)| {
                key.hash(hasher);
                hash_value(value, hasher);
            }),
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (key, value) in object {
        key.hash(&mut hasher);
        hash_value(value, &mut hasher);
    }
    hasher.finish()
}
