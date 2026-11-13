//! The terminal frame of the bound thread's Turn.

use serde_json::{Map, Value};

use crate::acp::lifecycle::{ItemLifecycle, TerminalUpdate, TurnState};

use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError,
    support::{error_payload, identity, text},
};
use super::{hooks::cancelled_hooks, plan};

pub(super) fn turn_completed(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let thread_id = text(params, "threadId", 512)?;
    adapter.expect_thread(thread_id).map_err(identity)?;
    let turn = params
        .get("turn")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let external = text(turn, "id", 512)?;
    adapter.expect_turn(external).map_err(identity)?;
    let state = match text(turn, "status", 32)? {
        "completed" => TurnState::Completed,
        "interrupted" if adapter.stopped_by_user(external) => TurnState::Cancelled,
        "interrupted" => TurnState::Interrupted,
        "failed" => TurnState::Failed,
        _ => return Err(IngressError::Malformed),
    };
    let terminal_error = if state == TurnState::Failed {
        turn.get("error")
            .filter(|value| !value.is_null())
            .map(|value| {
                let error = value.as_object().ok_or(IngressError::Malformed)?;
                let message = text(
                    error,
                    "message",
                    crate::acp::limits::LIMITS.wire.error_bytes as usize,
                )?;
                let payload = error_payload("codex", message, false)?;
                Ok::<_, IngressError>(
                    (!adapter.has_unretried_error(external, message)).then_some(payload),
                )
            })
            .transpose()?
            .flatten()
    } else {
        None
    };
    // Only a completed Turn closes its plan; the turn sweep cancels it otherwise.
    let plan = (state == TurnState::Completed)
        .then(|| adapter.turn_plan(external))
        .flatten();
    match adapter
        .accept_turn_terminal(external, state)
        .map_err(identity)?
    {
        TerminalUpdate::Apply => {
            adapter.clear_command_items();
            adapter.clear_running_mcp_tools();
            let id = adapter.scoped_turn(external).map_err(identity)?;
            let mut events = cancelled_hooks(adapter, thread_id)?;
            if let Some(entries) = plan {
                events.push(plan::event(
                    adapter,
                    external,
                    entries,
                    ItemLifecycle::Completed,
                )?);
            }
            if let Some(payload) = terminal_error {
                events.push(IncomingEvent::Error(payload));
            }
            events.push(IncomingEvent::Turn { id, state });
            Ok(IngressDisposition::Events(events))
        }
        TerminalUpdate::NoOp => Ok(IngressDisposition::DuplicateTerminal),
        TerminalUpdate::Conflict => Err(IngressError::TerminalConflict),
    }
}
