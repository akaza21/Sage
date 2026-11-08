use serde_json::{Map, Value};

use crate::acp::lifecycle::{ItemLifecycle, ServerRequestState, TerminalUpdate, TurnState};

use super::super::state::{AdapterError, ExternalKind, TurnStartFailure, TurnStartedRejection};
use super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ResourceMapper, normalize_item,
    record_unprojected,
    support::{
        error_payload, expect_thread_and_turn, identity, parse_usage, request_id_text, text,
    },
};

mod child_thread;
pub(super) use child_thread::item_event as child_item_event;
pub(super) use child_thread::turn_completed as child_turn_completed;
mod deltas;
mod items;
use items::item_event;
mod notices;
mod plan;
mod provisional;
use deltas::{DeltaKind, command_output_delta, delta_event, summary_part_added};
use plan::structured_plan;

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
            adapter
                .accept_thread_started(text(thread, "id", 512)?)
                .map_err(identity)?;
            Ok(IngressDisposition::IgnoredLate)
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
            adapter.goal_cleared();
            Ok(IngressDisposition::IgnoredLate)
        }
        "mcpServer/startupStatus/updated" => {
            let name = text(params, "name", 512)?;
            let thread = text(params, "threadId", 512)?;
            match text(params, "status", 32)? {
                "starting" | "ready" | "cancelled"
                    if params.get("error").is_some_and(Value::is_null)
                        && params.get("failureReason").is_some_and(Value::is_null) => {}
                "failed"
                    if params.get("failureReason").and_then(Value::as_str)
                        == Some("reauthenticationRequired") =>
                {
                    return notices::mcp_sign_in_required(adapter, name, thread);
                }
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
            if binding == super::super::state::TurnBinding::Provisional {
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
        "item/reasoning/summaryTextDelta" => delta_event(adapter, params, DeltaKind::Thought, None),
        // Codex shows the reasoning summary only; raw reasoning text is
        // validated for identity and never projected.
        "item/reasoning/textDelta" => {
            expect_thread_and_turn(adapter, params)?;
            Ok(IngressDisposition::DroppedRawReasoning)
        }
        "item/reasoning/summaryPartAdded" => summary_part_added(adapter, params),
        "item/plan/delta" => delta_event(adapter, params, DeltaKind::Plan, None),
        "turn/plan/updated" => structured_plan(adapter, params),
        "thread/goal/updated" => {
            adapter
                .expect_thread(text(params, "threadId", 512)?)
                .map_err(identity)?;
            if adapter.goal_supported() {
                adapter.goal_updated(params.get("goal").unwrap_or(&Value::Null));
            } else {
                record_unprojected(method);
            }
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
                turn_id: adapter.scoped_turn(turn).map_err(identity)?,
                payload: parse_usage(params)?,
            }
            .into())
        }
        "error" => {
            let turn = expect_thread_and_turn(adapter, params)?;
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
            let payload = error_payload("codex", message, will_retry)?;
            if !will_retry {
                adapter
                    .remember_unretried_error(turn, message)
                    .map_err(identity)?;
            }
            Ok(IncomingEvent::Error(payload).into())
        }
        "warning" => notices::warning(adapter, params),
        "guardianWarning" => notices::guardian_warning(adapter, params),
        "configWarning" => notices::config_warning(adapter, params),
        "deprecationNotice" => notices::deprecation_notice(adapter, params),
        "model/rerouted" => notices::model_rerouted(adapter, params),
        "model/verification" => notices::model_verification(adapter, params),
        "model/safetyBuffering/updated" => notices::model_safety_buffering(adapter, params),
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

fn turn_started_rejection(adapter: &CodexAdapter, error: AdapterError) -> IngressError {
    let reason = TurnStartedRejection::from_adapter_error(&error);
    if super::reports_diagnostics() {
        tracing::warn!(
            acp_turn_started_rejection = reason.as_str(),
            active_turn = adapter.has_active_turn(),
            pending_turn_start = adapter.has_pending_turn_start(),
            previous_turn = adapter.has_previous_turn(),
            "Codex turn/started rejected by adapter state"
        );
    }
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
    match adapter
        .accept_turn_terminal(external, state)
        .map_err(identity)?
    {
        TerminalUpdate::Apply => {
            adapter.clear_command_items();
            let id = adapter.scoped_turn(external).map_err(identity)?;
            let mut events = Vec::with_capacity(2);
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
