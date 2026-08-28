use serde_json::{Map, Value};

use crate::acp::{
    contract::SafeText,
    intent::NormalizedPayload,
    lifecycle::{ItemLifecycle, TerminalUpdate, TurnState},
};

use super::super::state::ExternalKind;
use super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode, ResourceMapper,
    content, error_payload, expect_thread_and_turn, identity, normalize_item, parse_usage,
    sanitize, text, unsupported,
};

pub(super) fn notification<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    method: &str,
    params: &Map<String, Value>,
    mapper: &M,
) -> Result<IngressDisposition, IngressError> {
    match method {
        "thread/started" => {
            let thread = params
                .get("thread")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            match adapter
                .accept_thread_started(text(thread, "id", 512)?)
                .map_err(identity)?
            {
                Some(id) => Ok(IncomingEvent::SessionReady { id }.into()),
                None => Ok(IngressDisposition::IgnoredLate),
            }
        }
        "remoteControl/status/changed" => {
            let _installation = text(params, "installationId", 512)?;
            let _server = text(params, "serverName", 512)?;
            match text(params, "status", 32)? {
                "disabled" | "enabled" => Ok(IngressDisposition::IgnoredLate),
                _ => Err(IngressError::Malformed),
            }
        }
        "mcpServer/startupStatus/updated" => {
            let _name = text(params, "name", 512)?;
            let _thread = text(params, "threadId", 512)?;
            if !params.get("error").is_some_and(Value::is_null)
                || !params.get("failureReason").is_some_and(Value::is_null)
            {
                return Err(IngressError::Malformed);
            }
            match text(params, "status", 32)? {
                "starting" | "ready" | "cancelled" => {}
                _ => return Err(IngressError::Malformed),
            }
            Ok(IngressDisposition::IgnoredLate)
        }
        "turn/started" => {
            adapter
                .expect_thread(text(params, "threadId", 512)?)
                .map_err(identity)?;
            let turn = params
                .get("turn")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let (id, binding) = adapter
                .observe_turn_started(text(turn, "id", 512)?)
                .map_err(identity)?;
            if binding == super::super::state::TurnBinding::PreviousTerminal {
                return Ok(IngressDisposition::IgnoredLate);
            }
            Ok(IncomingEvent::Turn {
                id,
                state: TurnState::Running,
            }
            .into())
        }
        "turn/completed" => turn_completed(adapter, params),
        "item/started" => item_event(adapter, params, ItemLifecycle::Started, mapper),
        "item/completed" => item_event(adapter, params, ItemLifecycle::Completed, mapper),
        "item/agentMessage/delta" => delta_event(adapter, params, DeltaKind::Text),
        "item/reasoning/summaryTextDelta" => delta_event(adapter, params, DeltaKind::Thought),
        "item/plan/delta" => delta_event(adapter, params, DeltaKind::Plan),
        "thread/tokenUsage/updated" => {
            expect_thread_and_turn(adapter, params)?;
            let turn = text(params, "turnId", 512)?;
            Ok(IncomingEvent::Usage {
                turn_id: adapter
                    .scoped_id(ExternalKind::Turn, turn)
                    .map_err(identity)?,
                payload: parse_usage(params)?,
            }
            .into())
        }
        "error" => {
            expect_thread_and_turn(adapter, params)?;
            let will_retry = params
                .get("willRetry")
                .and_then(Value::as_bool)
                .ok_or(IngressError::Malformed)?;
            let error = params
                .get("error")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let message = text(
                error,
                "message",
                crate::acp::limits::LIMITS.wire.error_bytes as usize,
            )?;
            Ok(IncomingEvent::Error(error_payload("codex", message, will_retry)?).into())
        }
        other => {
            Ok(IncomingEvent::Unsupported(unsupported(other, "Unsupported Codex event")?).into())
        }
    }
}

fn turn_completed(
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
        "interrupted" => TurnState::Interrupted,
        "failed" => TurnState::Failed,
        _ => return Err(IngressError::Malformed),
    };
    match adapter
        .accept_turn_terminal(external, state)
        .map_err(identity)?
    {
        TerminalUpdate::Apply => {
            let id = adapter
                .scoped_in_turn(ExternalKind::Turn, external, external)
                .map_err(identity)?;
            Ok(IncomingEvent::Turn { id, state }.into())
        }
        TerminalUpdate::NoOp => Ok(IngressDisposition::DuplicateTerminal),
        TerminalUpdate::Conflict => Err(IngressError::TerminalConflict),
    }
}

fn item_event<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    requested: ItemLifecycle,
    mapper: &M,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let external = text(item, "id", 512)?;
    if adapter.turn_is_terminal(turn) && !adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let Some((payload, tool_state, terminal)) = normalize_item(item, requested, mapper)? else {
        return Ok(IngressDisposition::Events(vec![]));
    };
    let lifecycle = terminal.unwrap_or(requested);
    if lifecycle.is_terminal() {
        match adapter
            .accept_item_terminal(turn, external, lifecycle)
            .map_err(identity)?
        {
            TerminalUpdate::NoOp => return Ok(IngressDisposition::DuplicateTerminal),
            TerminalUpdate::Conflict => return Err(IngressError::TerminalConflict),
            TerminalUpdate::Apply => {}
        }
    }
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    Ok(IncomingEvent::Item {
        id,
        lifecycle,
        tool_state,
        payload,
        update_mode: ItemUpdateMode::Replace,
    }
    .into())
}

enum DeltaKind {
    Text,
    Thought,
    Plan,
}

fn delta_event(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    kind: DeltaKind,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    if adapter.turn_is_terminal(turn) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let external = text(params, "itemId", 512)?;
    if adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let value = sanitize(text(params, "delta", 262_144)?);
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
