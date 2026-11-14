use serde_json::{Map, Value};

use sage_acp_protocol::intent::ExternalId;
use sage_acp_protocol::lifecycle::ItemLifecycle;
use sage_acp_protocol::lifecycle::TurnState;

use super::super::support::text;
use super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ResourceMapper, normalize_item,
};

pub(super) fn native_id(
    item: &Map<String, Value>,
    params: &Map<String, Value>,
) -> Result<Option<ExternalId>, IngressError> {
    if item.get("type").and_then(Value::as_str) != Some("subAgentActivity") {
        return Ok(None);
    }
    item.get("agentThreadId")
        .map(|_| {
            let child = text(item, "agentThreadId", 512)?;
            if child == text(params, "threadId", 512)? {
                return Err(IngressError::Identity);
            }
            ExternalId::new("childThreadId", child.to_owned()).map_err(|_| IngressError::Identity)
        })
        .transpose()
}

pub(in crate::acp::bindings::codex::incoming) fn item_event<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    method: &str,
    mapper: &M,
) -> Result<IngressDisposition, IngressError> {
    let child = text(params, "threadId", 512)?;
    if !adapter.is_bound_child_thread(child) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let turn = text(params, "turnId", 512)?;
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let item_id = text(item, "id", 512)?;
    if !matches!(
        item.get("type").and_then(Value::as_str),
        Some("commandExecution" | "fileChange" | "reasoning" | "agentMessage" | "userMessage")
    ) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let requested = if method == "item/completed" {
        ItemLifecycle::Completed
    } else {
        ItemLifecycle::Started
    };
    let kind = item.get("type").and_then(Value::as_str);
    let normalized = if kind == Some("userMessage") {
        Some((
            super::super::super::normalize_child_user_message(item)
                .map_err(|_| IngressError::Malformed)?,
            None,
            None,
        ))
    } else {
        normalize_item(item, requested, mapper)?
    };
    let Some((payload, tool_state, terminal)) = normalized else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    Ok(IncomingEvent::ChildLiveItem {
        child_thread_id: ExternalId::new("childThreadId", child.to_owned())
            .map_err(|_| IngressError::Identity)?,
        child_turn_id: ExternalId::new("childTurnId", turn.to_owned())
            .map_err(|_| IngressError::Identity)?,
        item_id: ExternalId::new("childItemId", item_id.to_owned())
            .map_err(|_| IngressError::Identity)?,
        role: match kind {
            Some("userMessage") => Some(sage_acp_protocol::codex_protocol::ChildTextRole::User),
            Some("agentMessage") => {
                Some(sage_acp_protocol::codex_protocol::ChildTextRole::Assistant)
            }
            _ => None,
        },
        lifecycle: terminal.unwrap_or(requested),
        tool_state,
        payload,
    }
    .into())
}

pub(in crate::acp::bindings::codex::incoming) fn turn_completed(
    adapter: &CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let child = text(params, "threadId", 512)?;
    if !adapter.is_bound_child_thread(child) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let turn = params
        .get("turn")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let status = match text(turn, "status", 32)? {
        "completed" => TurnState::Completed,
        "interrupted" => TurnState::Interrupted,
        "failed" => TurnState::Failed,
        _ => return Err(IngressError::Malformed),
    };
    Ok(IncomingEvent::ChildTurnStatus {
        child_thread_id: ExternalId::new("childThreadId", child.to_owned())
            .map_err(|_| IngressError::Identity)?,
        child_turn_id: ExternalId::new("childTurnId", text(turn, "id", 512)?.to_owned())
            .map_err(|_| IngressError::Identity)?,
        status,
    }
    .into())
}
