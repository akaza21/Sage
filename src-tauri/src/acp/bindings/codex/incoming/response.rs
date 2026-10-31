use super::*;
use crate::acp::bindings::codex::state::{TurnBinding, TurnStartFailure};

pub(super) fn response(
    adapter: &mut CodexAdapter,
    object: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let id = object
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    let kind = adapter.pending_kind(id).ok_or(IngressError::Uncorrelated)?;
    if kind == RequestKind::Models {
        let removed = adapter.take_pending(id);
        debug_assert_eq!(removed, Some(kind));
        return Ok(
            match object
                .get("result")
                .and_then(Value::as_object)
                .filter(|_| !object.contains_key("error"))
                .and_then(|result| parse_models(result).ok())
            {
                Some((models, next_cursor)) => IncomingEvent::Models {
                    models,
                    next_cursor,
                }
                .into(),
                None => IngressDisposition::IgnoredLate,
            },
        );
    }
    if let Some(error) = object.get("error") {
        if kind == RequestKind::Resume
            && (error.get("code").and_then(Value::as_i64).is_none()
                || error.get("message").and_then(Value::as_str).is_none()
                || object.contains_key("result")
                || object.contains_key("method"))
        {
            return Err(IngressError::Malformed);
        }
        if matches!(
            kind,
            RequestKind::GoalGet | RequestKind::GoalPause | RequestKind::GoalClear
        ) {
            let _ = error;
            if kind == RequestKind::GoalGet {
                adapter.goal_get_failed(id);
            } else {
                adapter.goal_mutation_failed(id);
            }
            let removed = adapter.take_pending(id);
            debug_assert_eq!(removed, Some(kind));
            return Ok(if kind == RequestKind::GoalGet {
                IngressDisposition::IgnoredLate
            } else {
                IngressDisposition::GoalMutationRejected
            });
        }
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex request failed");
        let payload = error_payload("codexRpc", message, false)?;
        if kind != RequestKind::TurnStart {
            let removed = adapter.take_pending(id);
            debug_assert_eq!(removed, Some(kind));
        }
        return match kind {
            RequestKind::Resume => Err(resume_error(error, message)),
            RequestKind::TurnStart => match adapter.fail_turn_start(id).map_err(identity)? {
                TurnStartFailure::Failed(turn) => Ok(IngressDisposition::Events(vec![
                    IncomingEvent::Error(payload),
                    IncomingEvent::Turn {
                        id: turn,
                        state: TurnState::Failed,
                    },
                ])),
                TurnStartFailure::Late => Ok(IngressDisposition::DuplicateTerminal),
            },
            RequestKind::Interrupt => {
                let turn = adapter.fail_interrupt().map_err(identity)?;
                Ok(IngressDisposition::Events(vec![
                    IncomingEvent::Error(payload),
                    IncomingEvent::Turn {
                        id: turn,
                        state: TurnState::Failed,
                    },
                ]))
            }
            RequestKind::Unsubscribe => Ok(IngressDisposition::Events(vec![
                IncomingEvent::Error(payload),
                IncomingEvent::Acknowledged {
                    closes_session: false,
                },
            ])),
            RequestKind::Steer => Ok(IngressDisposition::Events(vec![
                IncomingEvent::SteerResponse {
                    request_id: id,
                    accepted: false,
                },
                IncomingEvent::Error(payload),
            ])),
            _ => Ok(IncomingEvent::Error(payload).into()),
        };
    }
    if kind == RequestKind::GoalGet {
        let result = object.get("result").and_then(Value::as_object);
        adapter
            .goal_get_result(id, result.and_then(|result| result.get("goal")))
            .map_err(|_| IngressError::Malformed)?;
        let removed = adapter.take_pending(id);
        debug_assert_eq!(removed, Some(kind));
        return Ok(IngressDisposition::IgnoredLate);
    }
    if matches!(kind, RequestKind::GoalPause | RequestKind::GoalClear) {
        let accepted = adapter.goal_mutation_result(id, kind, object.get("result"));
        let removed = adapter.take_pending(id);
        debug_assert_eq!(removed, Some(kind));
        return Ok(if accepted {
            IngressDisposition::GoalMutationApplied
        } else {
            IngressDisposition::GoalMutationRejected
        });
    }
    let result = object
        .get("result")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    if kind != RequestKind::TurnStart {
        let removed = adapter.take_pending(id);
        debug_assert_eq!(removed, Some(kind));
    }
    let event = match kind {
        RequestKind::Initialize => IncomingEvent::Initialized {
            capabilities: adapter.accept_initialize(result).map_err(capability)?,
        },
        RequestKind::Start | RequestKind::Resume => {
            let thread = result
                .get("thread")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let external = text(thread, "id", 512)?;
            let id = adapter.bind_thread(external).map_err(identity)?;
            let current_model = result
                .get("model")
                .and_then(Value::as_str)
                .and_then(|value| RequiredText::new("model", value.to_owned()).ok());
            let current_effort = result
                .get("reasoningEffort")
                .and_then(Value::as_str)
                .and_then(|value| RequiredText::new("reasoningEffort", value.to_owned()).ok());
            IncomingEvent::SessionReady {
                id,
                current_model,
                current_effort,
            }
        }
        RequestKind::Read => {
            let thread = result
                .get("thread")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let external = text(thread, "id", 512)?;
            let id = adapter.bind_thread(external).map_err(identity)?;
            IncomingEvent::SessionLoaded { id }
        }
        RequestKind::TurnStart => {
            let turn = result
                .get("turn")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let external = text(turn, "id", 512)?;
            let (id, binding) = adapter
                .complete_turn_start(id, external)
                .map_err(identity)?;
            match binding {
                TurnBinding::PreviousTerminal => {
                    return Ok(IngressDisposition::DuplicateTerminal);
                }
                TurnBinding::Active => {
                    return Ok(IngressDisposition::IgnoredLate);
                }
                TurnBinding::Provisional => {
                    return Ok(IngressDisposition::IgnoredLate);
                }
                TurnBinding::NewlyActive => {}
            }
            IncomingEvent::Turn {
                id,
                state: TurnState::Sending,
            }
        }
        RequestKind::Interrupt => IncomingEvent::Acknowledged {
            closes_session: false,
        },
        RequestKind::Steer => {
            let turn_id = text(result, "turnId", 512)?;
            if !adapter.has_active_turn() || adapter.expect_turn(turn_id).is_err() {
                return Err(IngressError::Malformed);
            }
            IncomingEvent::SteerResponse {
                request_id: id,
                accepted: true,
            }
        }
        RequestKind::Unsubscribe => {
            adapter.clear_command_items();
            IncomingEvent::Acknowledged {
                closes_session: true,
            }
        }
        RequestKind::Models
        | RequestKind::GoalGet
        | RequestKind::GoalPause
        | RequestKind::GoalClear
        | RequestKind::ChildRead
        | RequestKind::Fork
        | RequestKind::ForkRelease
        | RequestKind::ForkDelete => {
            unreachable!("handled above")
        }
    };
    if matches!(
        kind,
        RequestKind::Start | RequestKind::Resume | RequestKind::Read
    ) {
        let external = result
            .get("thread")
            .and_then(Value::as_object)
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .ok_or(IngressError::Malformed)?;
        if adapter.take_browser_mcp_failure_for_thread(external) {
            return Ok(IngressDisposition::Events(vec![
                event,
                browser_mcp_unavailable()?,
            ]));
        }
    }
    Ok(event.into())
}

fn resume_error(error: &Value, message: &str) -> IngressError {
    if error["code"].as_i64() == Some(-32000)
        && [
            "authentication required",
            "not logged in",
            "not authenticated",
        ]
        .iter()
        .any(|known| message.eq_ignore_ascii_case(known))
    {
        return IngressError::SignInRequired;
    }
    // Codex's invalid-request response for a missing persisted rollout is
    // definitive. Generic JSON-RPC server/internal errors are not.
    if error["code"].as_i64() == Some(-32600)
        && message
            .strip_prefix("no rollout found for thread id ")
            .is_some_and(|id| !id.is_empty() && id.len() <= 512)
    {
        IngressError::ResumeRefused
    } else {
        IngressError::ActivationFailed
    }
}

pub(super) fn browser_mcp_unavailable() -> Result<IncomingEvent, IngressError> {
    Ok(IncomingEvent::Error(error_payload(
        "browserMcpUnavailable",
        "Browser tools are unavailable for this Codex chat. You can continue chatting, but browser actions will not work until a new chat starts with a working browser helper.",
        false,
    )?))
}
