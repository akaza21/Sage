use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::state::ExternalKind, contract::SafeText, intent::NormalizedPayload,
    lifecycle::ItemLifecycle,
};
use sage_core::limits::LIMITS;

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode,
    normalize_materialized_command,
    support::{content, expect_thread_and_turn, identity, sanitize, text},
};

pub(super) enum DeltaKind {
    Text,
    Thought,
    Plan,
}

pub(super) fn summary_part_added(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let index = params
        .get("summaryIndex")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    if index == 0 {
        expect_thread_and_turn(adapter, params)?;
        return Ok(IngressDisposition::IgnoredLate);
    }
    delta_event(adapter, params, DeltaKind::Thought, Some("\n\n"))
}

pub(super) fn command_output_delta(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    if adapter.turn_is_terminal(turn) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let external = text(params, "itemId", 512)?;
    if adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let delta = params
        .get("delta")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= LIMITS.wire.frame_bytes as usize && !value.contains('\0'))
        .ok_or(IngressError::Malformed)?;
    let command = adapter.append_command_output_delta(turn, external, delta)?;
    let (payload, tool_state, terminal) = normalize_materialized_command(&command)?;
    if terminal.is_some() || tool_state.is_none() {
        return Err(IngressError::Malformed);
    }
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    adapter.commit_command_item(turn, external, command)?;
    Ok(IncomingEvent::Item {
        id,
        lifecycle: ItemLifecycle::Streaming,
        tool_state,
        payload,
        update_mode: ItemUpdateMode::Replace,
    }
    .into())
}

pub(super) fn delta_event(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    kind: DeltaKind,
    literal: Option<&str>,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    if adapter.turn_is_terminal(turn) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let external = text(params, "itemId", 512)?;
    if adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let value = match literal {
        Some(literal) => literal.to_owned(),
        None => sanitize(text(params, "delta", 262_144)?),
    };
    let payload = match kind {
        DeltaKind::Text => NormalizedPayload::Text {
            text: SafeText::new("text", value).map_err(content)?,
        },
        DeltaKind::Thought => NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("summary", value).map_err(content)?,
        },
        DeltaKind::Plan => NormalizedPayload::Plan {
            summary: SafeText::new("plan", value).map_err(content)?,
        },
    };
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    Ok(IncomingEvent::Item {
        id,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload,
        update_mode: ItemUpdateMode::Append,
    }
    .into())
}
