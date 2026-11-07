use crate::acp::{
    intent::session_event::SessionScopedEvent,
    runtime::adapter_pump::{PumpIntent, PumpJournalError, SessionScope},
    store::{AdapterInboundEvent, SessionEventCommand},
};

use super::command::hex;

/// A session-scoped fact rides the current owner batch but is checked
/// against the session portion of the owner only; it names no Turn.
pub(super) fn event_bytes(
    intent: &PumpIntent,
    scope: &SessionScope,
    event: &SessionScopedEvent,
) -> Result<usize, PumpJournalError> {
    if scope.project_id != intent.project_id
        || scope.chat_id != intent.chat_id
        || scope.session_id != intent.session_id
        || scope.runtime_epoch != intent.runtime_epoch
        || scope.binding_digest != hex(&intent.binding_digest)
    {
        return Err(PumpJournalError::Rejected);
    }
    let event_bytes = serde_json::to_vec(event)
        .map_err(|_| PumpJournalError::Rejected)?
        .len();
    Ok(scope.project_id.as_str().len()
        + scope.chat_id.as_str().len()
        + scope.session_id.as_str().len()
        + scope.binding_digest.len()
        + event_bytes)
}

pub(super) fn convert(event: SessionScopedEvent, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand { event, now_millis })
}
