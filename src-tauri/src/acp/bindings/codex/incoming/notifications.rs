use serde_json::{Map, Value};

use crate::acp::lifecycle::{ItemLifecycle, ServerRequestState, TerminalUpdate, TurnState};

use super::super::state::{AdapterError, ExternalKind, TurnStartFailure, TurnStartedRejection};
use super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode, ResourceMapper,
    normalize_item, normalize_materialized_command, record_unprojected,
    support::{
        error_payload, expect_thread_and_turn, identity, parse_usage, request_id_text, text,
    },
};

mod deltas;
use deltas::{DeltaKind, command_output_delta, delta_event};

pub(super) fn notification<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    method: &str,
    params: &Map<String, Value>,
    mapper: &M,
    declarations: &crate::acp::declared_test::DeclaredTestCommands,
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
                Some(id) => {
                    let mut events = vec![IncomingEvent::SessionReady { id }];
                    if adapter.take_browser_mcp_failure_for_thread(text(thread, "id", 512)?) {
                        events.push(super::browser_mcp_unavailable()?);
                    }
                    Ok(IngressDisposition::Events(events))
                }
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
        // Account state belongs to Codex and has no Sage-owned projection.
        // The frame is bounded before reaching this matcher and its payload is
        // discarded, so a known account update cannot become a transcript
        // artifact or alter a turn/session state.
        "account/updated" | "account/rateLimits/updated" | "skills/changed" => {
            Ok(IngressDisposition::IgnoredLate)
        }
        "thread/status/changed" => {
            let thread = text(params, "threadId", 512)?;
            let status = params
                .get("status")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            match text(status, "type", 32)? {
                // Codex emits these informational transitions both immediately
                // after `thread/resume` and immediately after `turn/start`.
                // They carry no durable state Sage needs to project, but must
                // not tear down the live runner before `turn/started` arrives.
                "idle" => Ok(IngressDisposition::IgnoredLate),
                "active" => {
                    let flags = status
                        .get("activeFlags")
                        .and_then(Value::as_array)
                        .ok_or(IngressError::Malformed)?;
                    if flags.len() > 32
                        || flags.iter().any(|flag| {
                            !flag
                                .as_str()
                                .is_some_and(|value| !value.is_empty() && value.len() <= 64)
                        })
                    {
                        return Err(IngressError::Malformed);
                    }
                    Ok(IngressDisposition::IgnoredLate)
                }
                "systemError" => {
                    adapter.expect_thread(thread).map_err(identity)?;
                    let payload = error_payload(
                        "codex",
                        "Codex reported a system error while starting the turn. Check the selected model or harness connection, then retry.",
                        false,
                    )?;
                    match adapter.fail_pending_turn_start().map_err(identity)? {
                        TurnStartFailure::Failed(turn) => Ok(IngressDisposition::Events(vec![
                            IncomingEvent::Error(payload),
                            IncomingEvent::Turn {
                                id: turn,
                                state: TurnState::Failed,
                            },
                        ])),
                        TurnStartFailure::Late => Ok(IngressDisposition::DuplicateTerminal),
                    }
                }
                _ => Err(IngressError::Malformed),
            }
        }
        "thread/goal/cleared" => {
            // Codex emits this one-field informational notification while a
            // resumed thread is rehydrated. It changes Codex-local goal state,
            // which Sage neither owns nor projects, but must not abort the
            // ReadyHandoff before the next turn can start.
            if params.len() != 1 {
                return Err(IngressError::Malformed);
            }
            adapter
                .expect_thread(text(params, "threadId", 512)?)
                .map_err(identity)?;
            Ok(IngressDisposition::IgnoredLate)
        }
        "mcpServer/startupStatus/updated" => {
            let name = text(params, "name", 512)?;
            let thread = text(params, "threadId", 512)?;
            match text(params, "status", 32)? {
                "starting" | "ready" | "cancelled"
                    if params.get("error").is_some_and(Value::is_null)
                        && params.get("failureReason").is_some_and(Value::is_null) => {}
                "failed" if name == "sage-browser" => {
                    text(
                        params,
                        "error",
                        crate::acp::limits::LIMITS.wire.error_bytes as usize,
                    )?;
                    if !params.get("failureReason").is_some_and(Value::is_null) {
                        return Err(IngressError::Malformed);
                    }
                    if !adapter.has_bound_thread() {
                        adapter.buffer_browser_mcp_failure(thread);
                        return Ok(IngressDisposition::IgnoredLate);
                    }
                    adapter.expect_thread(thread).map_err(identity)?;
                    if !adapter.has_active_turn() {
                        adapter.buffer_browser_mcp_failure(thread);
                        return Ok(IngressDisposition::IgnoredLate);
                    }
                    if !adapter.mark_browser_mcp_failure_reported() {
                        return Ok(IngressDisposition::IgnoredLate);
                    }
                    return Ok(super::browser_mcp_unavailable()?.into());
                }
                _ => return Err(IngressError::Malformed),
            }
            Ok(IngressDisposition::IgnoredLate)
        }
        "turn/started" => {
            let thread = text(params, "threadId", 512)?;
            if let Err(error) = adapter.expect_thread(thread) {
                return Err(turn_started_rejection(adapter, error));
            }
            let turn = params
                .get("turn")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let turn_id = text(turn, "id", 512)?;
            let (id, binding) = match adapter.observe_turn_started(turn_id) {
                Ok(value) => value,
                Err(error) => return Err(turn_started_rejection(adapter, error)),
            };
            if binding == super::super::state::TurnBinding::PreviousTerminal {
                return Ok(IngressDisposition::IgnoredLate);
            }
            let mut events = vec![IncomingEvent::Turn {
                id,
                state: TurnState::Running,
            }];
            if adapter.take_browser_mcp_failure_for_thread(thread) {
                events.push(super::browser_mcp_unavailable()?);
            }
            Ok(IngressDisposition::Events(events))
        }
        "turn/completed" => turn_completed(adapter, params),
        "item/started" => item_event(
            adapter,
            params,
            ItemLifecycle::Started,
            mapper,
            declarations,
        ),
        "item/completed" => item_event(
            adapter,
            params,
            ItemLifecycle::Completed,
            mapper,
            declarations,
        ),
        "item/commandExecution/outputDelta" => command_output_delta(adapter, params),
        "item/agentMessage/delta" => delta_event(adapter, params, DeltaKind::Text, None),
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            delta_event(adapter, params, DeltaKind::Thought, None)
        }
        "item/reasoning/summaryPartAdded" => summary_part_added(adapter, params),
        "item/plan/delta" => delta_event(adapter, params, DeltaKind::Plan, None),
        "thread/goal/updated" => {
            adapter
                .expect_thread(text(params, "threadId", 512)?)
                .map_err(identity)?;
            record_unprojected(method);
            Ok(IngressDisposition::IgnoredLate)
        }
        "serverRequest/resolved" => server_request_resolved(adapter, params),
        "thread/tokenUsage/updated" => {
            let thread = text(params, "threadId", 512)?;
            adapter.expect_thread(thread).map_err(identity)?;
            let turn = text(params, "turnId", 512)?;
            if adapter.expect_turn(turn).is_err() {
                // Resume can replay bounded usage for a historical, terminal
                // turn before this new adapter owns an active turn. Validate
                // the payload but do not project uncorrelated history.
                if !adapter.has_active_turn() {
                    let _ = parse_usage(params)?;
                    return Ok(IngressDisposition::IgnoredLate);
                }
                return Err(IngressError::Identity);
            }
            Ok(IncomingEvent::Usage {
                turn_id: adapter
                    .scoped_in_turn(ExternalKind::Turn, turn, turn)
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
            record_unprojected(other);
            Ok(IngressDisposition::IgnoredLate)
        }
    }
}

fn server_request_resolved(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    adapter
        .expect_thread(text(params, "threadId", 512)?)
        .map_err(identity)?;
    let request_id = request_id_text(params.get("requestId").ok_or(IngressError::Malformed)?)?;
    match adapter
        .resolve_external_permission(&request_id)
        .map_err(identity)?
    {
        Some((turn_id, external_request)) => Ok(IncomingEvent::PermissionResolved {
            turn_id,
            external_request,
            state: ServerRequestState::Cancelled,
        }
        .into()),
        None => Ok(IngressDisposition::IgnoredLate),
    }
}

fn summary_part_added(
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

fn turn_started_rejection(adapter: &CodexAdapter, error: AdapterError) -> IngressError {
    let reason = TurnStartedRejection::from_adapter_error(&error);
    tracing::warn!(
        acp_turn_started_rejection = reason.as_str(),
        active_turn = adapter.has_active_turn(),
        pending_turn_start = adapter.has_pending_turn_start(),
        previous_turn = adapter.has_previous_turn(),
        "Codex turn/started rejected by adapter state"
    );
    identity(error)
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
            adapter.clear_command_items();
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
    declarations: &crate::acp::declared_test::DeclaredTestCommands,
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
    let command = (item.get("type").and_then(Value::as_str) == Some("commandExecution"))
        .then(|| {
            adapter.materialize_command_item(
                turn,
                external,
                item,
                mapper,
                declarations,
                requested.is_terminal(),
            )
        })
        .transpose()?;
    let normalized = match command.as_ref() {
        Some(command) => Some(normalize_materialized_command(command)?),
        None => normalize_item(item, requested, mapper)?,
    };
    let Some((mut payload, tool_state, terminal)) = normalized else {
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
    if let Some(command) = command {
        if lifecycle.is_terminal() {
            adapter.remove_command_item(external);
        } else {
            adapter.commit_command_item(turn, external, command)?;
        }
    }
    adapter.enrich_subagent_payload(&mut payload);
    adapter.learn_child_threads(&payload);
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
