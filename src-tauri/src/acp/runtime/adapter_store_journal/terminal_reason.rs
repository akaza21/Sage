use crate::acp::{
    lifecycle::TurnTerminalReason,
    runtime::adapter_pump::{PumpIntent, PumpJournalError, TurnScope},
    store::{AdapterInboundEvent, TurnTerminalReasonCommand},
};

const MESSAGE_BYTES: usize = 8192;

pub(super) fn event_bytes(
    intent: &PumpIntent,
    scope: &TurnScope,
    message: Option<&String>,
    remediation: Option<&String>,
) -> Result<usize, PumpJournalError> {
    super::command::verify_scope(intent, scope)?;
    let message_bytes = message.map_or(0, String::len) + remediation.map_or(0, String::len);
    if message.map_or(0, String::len) > MESSAGE_BYTES
        || remediation.map_or(0, String::len) > MESSAGE_BYTES
    {
        return Err(PumpJournalError::Rejected);
    }
    Ok(super::command::scope_heap_bytes(scope) + message_bytes)
}

pub(super) fn convert(
    reason: TurnTerminalReason,
    message: Option<String>,
    remediation: Option<String>,
    now_millis: i64,
) -> AdapterInboundEvent {
    AdapterInboundEvent::TerminalReason(TurnTerminalReasonCommand {
        reason,
        message,
        remediation,
        now_millis,
    })
}
