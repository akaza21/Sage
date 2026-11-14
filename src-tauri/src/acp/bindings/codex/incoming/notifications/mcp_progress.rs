use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::state::ExternalKind,
    contract::SafeText,
    intent::TOOL_PROGRESS_BYTES,
    lifecycle::{ItemLifecycle, ToolState},
};
use sage_core::limits::LIMITS;

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode,
    support::{content, expect_thread_and_turn, identity, text},
};
use super::status_line::status_line;

pub(super) fn mcp_tool_progress(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    let external = text(params, "itemId", 512)?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= LIMITS.wire.frame_bytes as usize && !value.contains('\0'))
        .ok_or(IngressError::Malformed)?;
    if adapter.turn_is_terminal(turn) || adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let progress = status_line(message, TOOL_PROGRESS_BYTES)
        .map(|line| SafeText::new("mcpProgress", line))
        .transpose()
        .map_err(content)?;
    let Some(payload) = adapter.report_mcp_tool_progress(turn, external, progress) else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    Ok(IncomingEvent::Item {
        id,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: Some(ToolState::InProgress),
        payload,
        update_mode: ItemUpdateMode::Replace,
    }
    .into())
}
