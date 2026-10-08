use serde_json::{Map, Value};

use crate::acp::intent::ExternalId;
use crate::acp::lifecycle::ItemLifecycle;

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
        Some("commandExecution" | "fileChange" | "reasoning")
    ) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let requested = if method == "item/completed" {
        ItemLifecycle::Completed
    } else {
        ItemLifecycle::Started
    };
    let Some((payload, tool_state, terminal)) = normalize_item(item, requested, mapper)? else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    Ok(IncomingEvent::ChildLiveItem {
        child_thread_id: ExternalId::new("childThreadId", child.to_owned())
            .map_err(|_| IngressError::Identity)?,
        child_turn_id: ExternalId::new("childTurnId", turn.to_owned())
            .map_err(|_| IngressError::Identity)?,
        item_id: ExternalId::new("childItemId", item_id.to_owned())
            .map_err(|_| IngressError::Identity)?,
        lifecycle: terminal.unwrap_or(requested),
        tool_state,
        payload,
    }
    .into())
}
