use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::state::{ExternalKind, PlanEntries},
    contract::{BoundedVec, SafeText},
    intent::{NormalizedPayload, NormalizedPlanEntry, NormalizedPlanState},
    lifecycle::ItemLifecycle,
};

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode,
    support::{content, expect_thread_and_turn, identity, text},
};

pub(super) fn structured_plan(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    if adapter.turn_is_terminal(turn) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let raw = params
        .get("plan")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if raw.len() > 128 {
        return Err(IngressError::Malformed);
    }
    let mut entries = Vec::with_capacity(raw.len());
    for entry in raw {
        let entry = entry.as_object().ok_or(IngressError::Malformed)?;
        let state = match text(entry, "status", 32)? {
            "pending" => NormalizedPlanState::Pending,
            "inProgress" => NormalizedPlanState::InProgress,
            "completed" => NormalizedPlanState::Completed,
            _ => return Err(IngressError::Malformed),
        };
        entries.push(NormalizedPlanEntry {
            label: SafeText::new("planStep", text(entry, "step", 512)?).map_err(content)?,
            state,
        });
    }
    let entries = BoundedVec::new("planEntries", entries).map_err(content)?;
    adapter.remember_turn_plan(entries.clone());
    Ok(event(adapter, turn, entries, ItemLifecycle::Streaming)?.into())
}

/// The Turn's one plan item; every update replaces the whole list.
pub(super) fn event(
    adapter: &CodexAdapter,
    turn: &str,
    entries: PlanEntries,
    lifecycle: ItemLifecycle,
) -> Result<IncomingEvent, IngressError> {
    Ok(IncomingEvent::Item {
        id: adapter
            .scoped_in_turn(ExternalKind::Item, turn, "sage:turn-plan")
            .map_err(identity)?,
        lifecycle,
        tool_state: None,
        payload: NormalizedPayload::PlanEntries { entries },
        update_mode: ItemUpdateMode::Replace,
    })
}
