//! Notifications about the bound thread itself rather than one of its Turns.

use serde_json::{Map, Value};

use crate::acp::bindings::session_title::harness_title_event;

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError,
    support::{identity, text},
};

/// `thread/name/updated`: Codex named (or renamed) the thread. A missing or
/// null name clears Codex's own name; Sage keeps the last title it showed.
pub(super) fn thread_name_updated(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    if params
        .keys()
        .any(|key| key != "threadId" && key != "threadName")
    {
        return Err(IngressError::Malformed);
    }
    let thread = text(params, "threadId", 512)?;
    let name = match params.get("threadName") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name.as_str()),
        Some(_) => return Err(IngressError::Malformed),
    };
    if !adapter.has_bound_thread() {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let id = adapter.thread_session_id(thread).map_err(identity)?;
    Ok(match name.and_then(harness_title_event) {
        Some(event) => IngressDisposition::Events(vec![IncomingEvent::Session { id, event }]),
        None => IngressDisposition::IgnoredLate,
    })
}
