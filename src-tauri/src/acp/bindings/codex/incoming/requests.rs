use std::hash::{Hash, Hasher};

use serde_json::{Map, Value, json};

use crate::acp::runtime::host_browse::admit_browse_url;
use sage_acp_protocol::{
    contract::SafeText,
    intent::{NormalizedPayload, ToolCategory, user_input::UserInputRequest},
};
use sage_core::limits::LIMITS;

use super::super::CODEX_BROWSE_TOOL_NAME;
use super::super::state::{
    CodexAdapter, ExternalKind, PermissionRegistration, PermissionRequestParams, USER_INPUT_METHOD,
};
use super::{
    DenyResponse, IncomingEvent, IngressDisposition, IngressError, record_unprojected,
    support::{identity, is_browse_alias, permission, request_id_text, sanitize, text},
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
    let id = object.get("id").ok_or(IngressError::Malformed)?;
    if method == USER_INPUT_METHOD {
        return user_input::request(adapter, object, params);
    }
    if method == elicitation::ELICITATION_METHOD {
        return elicitation::request(adapter, object, params);
    }
    if method == "currentTime/read" {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let bytes = reply(id, "result", json!({"currentTimeAt": now}))?;
        return Ok(IncomingEvent::HostToolReply { bytes }.into());
    }
    let Some(category) = approval_category(method) else {
        return unsupported_request(id, method);
    };
    let thread = text(params, "threadId", 512)?;
    let turn = text(params, "turnId", 512)?;
    let item = text(params, "itemId", 512)?;
    adapter.expect_thread(thread).map_err(identity)?;
    adapter.expect_turn(turn).map_err(identity)?;
    let result = if method == "item/permissions/requestApproval" {
        json!({"permissions": {}})
    } else {
        json!({"decision": "decline"})
    };
    let bytes = reply(id, "result", result)?;
    let offered = offered_approval_kinds(method, params);
    let operation = match method {
        "item/commandExecution/requestApproval" => {
            SafeText::new("permissionSummary", "Run a command").expect("fixed summary")
        }
        "item/fileChange/requestApproval" => {
            SafeText::new("permissionSummary", "Change files").expect("fixed summary")
        }
        _ => SafeText::new("permissionSummary", "Request additional permissions")
            .expect("fixed summary"),
    };
    let allow_always =
        offered.contains(&sage_acp_protocol::intent::PermissionOptionKind::AllowAlways);
    let payload = NormalizedPayload::PermissionRequest {
        category,
        summary: operation,
        offered_option_kinds: Some(
            sage_acp_protocol::contract::BoundedVec::new("permissionOptionKinds", offered)
                .expect("fixed permission choices"),
        ),
        reason: approval_reason(params),
        user_input: None,
    };
    project_server_request(
        adapter,
        PendingServerRequest {
            object,
            method,
            allow_always,
            turn,
            item,
            bytes,
            payload,
            form: None,
        },
    )
}

/// One harness server request that waits for the user, with the native
/// reply that declines it (sent on cancel, interrupt or auto-resolution).
struct PendingServerRequest<'a> {
    object: &'a Map<String, Value>,
    method: &'a str,
    allow_always: bool,
    turn: &'a str,
    item: &'a str,
    bytes: Vec<u8>,
    payload: NormalizedPayload,
    /// The form an elicitation asked, kept to type the accepted content.
    form: Option<Box<UserInputRequest>>,
}

fn project_server_request(
    adapter: &mut CodexAdapter,
    request: PendingServerRequest<'_>,
) -> Result<IngressDisposition, IngressError> {
    let PendingServerRequest {
        object,
        method,
        allow_always,
        turn,
        item,
        bytes,
        payload,
        form,
    } = request;
    let request_id = request_id_text(object.get("id").ok_or(IngressError::Malformed)?)?;
    let external_request = match adapter
        .register_permission_request(PermissionRequestParams {
            request_id: &request_id,
            fingerprint: request_fingerprint(object),
            method,
            allow_always,
            turn_id: turn,
            item_id: item,
            response: &bytes,
            form,
        })
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
    let event = IncomingEvent::Permission {
        turn_id: adapter.scoped_turn(turn).map_err(identity)?,
        item_id: adapter
            .scoped_in_turn(ExternalKind::Item, turn, item)
            .map_err(identity)?,
        payload,
        denial: Box::new(DenyResponse {
            external_request,
            bytes,
        }),
    };
    Ok(event.into())
}

mod elicitation;
mod user_input;

fn offered_approval_kinds(
    method: &str,
    params: &Map<String, Value>,
) -> Vec<sage_acp_protocol::intent::PermissionOptionKind> {
    use sage_acp_protocol::intent::PermissionOptionKind::{AllowAlways, AllowOnce, RejectOnce};
    if method == "item/permissions/requestApproval" {
        return vec![AllowOnce, RejectOnce];
    }
    if method == "item/commandExecution/requestApproval" {
        return match params.get("availableDecisions") {
            None | Some(Value::Null) => vec![AllowOnce, AllowAlways, RejectOnce],
            Some(Value::Array(decisions)) => {
                let offered: Vec<_> = decisions
                    .iter()
                    .filter_map(|decision| match decision.as_str()? {
                        "accept" => Some(AllowOnce),
                        "acceptForSession" => Some(AllowAlways),
                        "decline" => Some(RejectOnce),
                        _ => None,
                    })
                    .take(8)
                    .collect();
                if offered.is_empty() {
                    vec![RejectOnce]
                } else {
                    offered
                }
            }
            Some(_) => vec![RejectOnce],
        };
    }
    if method == "item/fileChange/requestApproval" {
        return vec![AllowOnce, AllowAlways, RejectOnce];
    }
    vec![AllowOnce, RejectOnce]
}

fn approval_reason(params: &Map<String, Value>) -> Option<SafeText<2048>> {
    let reason = sanitize(params.get("reason")?.as_str()?.trim());
    let mut end = reason.len().min(2048);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    let reason = reason[..end].trim();
    (!reason.is_empty())
        .then(|| SafeText::new("permissionReason", reason).ok())
        .flatten()
}

pub(in crate::acp::bindings::codex) fn approval_category(method: &str) -> Option<ToolCategory> {
    match method {
        "item/commandExecution/requestApproval" => Some(ToolCategory::Execute),
        "item/fileChange/requestApproval" => Some(ToolCategory::Edit),
        "item/permissions/requestApproval" => Some(ToolCategory::Other),
        _ => None,
    }
}

fn unsupported_request(id: &Value, method: &str) -> Result<IngressDisposition, IngressError> {
    record_unprojected(method);
    let bytes = reply(
        id,
        "error",
        json!({"code": -32601, "message": "Sage does not implement this Codex server request"}),
    )?;
    Ok(IncomingEvent::HostToolReply { bytes }.into())
}

fn reply(id: &Value, field: &str, body: Value) -> Result<Vec<u8>, IngressError> {
    let mut frame = Map::new();
    frame.insert("id".to_owned(), id.clone());
    frame.insert(field.to_owned(), body);
    let bytes = serde_json::to_vec(&Value::Object(frame)).map_err(|_| IngressError::Malformed)?;
    if bytes.len() >= LIMITS.wire.frame_bytes as usize
        || bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r'))
    {
        return Err(IngressError::FrameBytes);
    }
    Ok(bytes)
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
    let id = object.get("id").ok_or(IngressError::Malformed)?;
    if is_browse_alias(tool) {
        return Err(IngressError::UnsupportedServerRequest);
    }
    if tool != CODEX_BROWSE_TOOL_NAME {
        return unavailable_tool(id, tool);
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
    let bytes = reply(
        id,
        "result",
        json!({
            "success": true,
            "contentItems": [{"type": "inputText", "text": href}]
        }),
    )?;
    Ok(IncomingEvent::HostToolReply { bytes }.into())
}

fn unavailable_tool(id: &Value, tool: &str) -> Result<IngressDisposition, IngressError> {
    record_unprojected(tool);
    let bytes = reply(
        id,
        "result",
        json!({
            "success": false,
            "contentItems": [{
                "type": "inputText",
                "text": "Sage does not provide this tool"
            }]
        }),
    )?;
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
