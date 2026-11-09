//! Child transcript snapshots built from captured wire, for tests.

use super::{
    ChildTranscriptSnapshot, NativeTurnsPage, child_transcript::ChildTranscriptError,
    normalize_turns_page,
};
use crate::acp::bindings::adapter::ResourceMapper;

/// The snapshot a read of `child` under `parent` keeps for `page`.
pub(crate) fn child_snapshot(
    page: NativeTurnsPage,
    child: &str,
    parent: &str,
) -> ChildTranscriptSnapshot {
    ChildTranscriptSnapshot {
        child_thread_id: child.into(),
        parent_thread_id: parent.into(),
        turns: page.turns,
        retention_limited: false,
    }
}

/// Normalizes a captured `thread/read` child transcript as the single
/// `thread/turns/list` page holding its Turns.
pub(crate) fn child_read<M: ResourceMapper + ?Sized>(
    wire: &[u8],
    request_id: u64,
    child: &str,
    parent: &str,
    mapper: &M,
) -> Result<ChildTranscriptSnapshot, ChildTranscriptError> {
    let read: serde_json::Value =
        serde_json::from_slice(wire).map_err(|_| ChildTranscriptError::Malformed)?;
    let page = serde_json::json!({"id": read["id"], "result": {"data": read["result"]["thread"]["turns"]}});
    let page = serde_json::to_vec(&page).map_err(|_| ChildTranscriptError::Malformed)?;
    Ok(child_snapshot(
        normalize_turns_page(&page, request_id, mapper)?,
        child,
        parent,
    ))
}
