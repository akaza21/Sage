//! `thread/compacted`: the legacy compaction event. It shows the same
//! compaction row the `contextCompaction` item does, unless that item
//! already showed this compaction (see `state/compactions.rs`).

use serde_json::{Map, Value};

use crate::acp::bindings::codex::state::ExternalKind;
use sage_acp_protocol::{
    intent::NormalizedPayload,
    lifecycle::{ItemLifecycle, TerminalUpdate},
};

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode,
    support::{expect_thread_and_turn, identity},
};

pub(super) fn thread_compacted(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    if adapter.turn_is_terminal(turn) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let Some(ordinal) = adapter.record_compaction(turn, true) else {
        return Ok(IngressDisposition::IgnoredLate);
    };
    // The event names no item, so the row's identity is its ordinal in the
    // Turn.
    let external = format!("thread-compacted-{ordinal}");
    let lifecycle = ItemLifecycle::Completed;
    if adapter
        .accept_item_terminal(turn, &external, lifecycle)
        .map_err(identity)?
        != TerminalUpdate::Apply
    {
        return Ok(IngressDisposition::DuplicateTerminal);
    }
    Ok(IncomingEvent::Item {
        id: adapter
            .scoped_in_turn(ExternalKind::Item, turn, &external)
            .map_err(identity)?,
        lifecycle,
        tool_state: None,
        payload: NormalizedPayload::CompactionNotice,
        update_mode: ItemUpdateMode::Replace,
    }
    .into())
}
