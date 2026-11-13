use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::acp::limits::LIMITS;

use super::super::FixtureError;
use super::super::interpreter::{field_str, field_u64};
use super::{WireState, acp, background, user_input, write_stdout};

pub(super) const THREAD_ID: &str = "fixture-persisted-session-1";
/// The session a native capture's fork creates (Codex `thread/fork` per the
/// pinned schema; Claude `session/fork` per claude-agent-acp 0.76.0).
pub(super) const FORK_THREAD_ID: &str = "fixture-fork-session-1";
const USER_AGENT: &str = concat!(
    "sage/",
    crate::acp::bindings::codex::pinned_codex_version!(),
    " (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"
);

pub(super) fn await_request(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    expected_method: &str,
) -> Result<(), FixtureError> {
    await_request_inner(input, output, state, expected_method, false)
}

pub(super) fn await_goal_with_optional_models(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    await_request_inner(input, output, state, "thread/goal/get", true)
}

fn await_request_inner(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    expected_method: &str,
    mut optional_models: bool,
) -> Result<(), FixtureError> {
    for _ in 0..32 {
        let Some(request) = read_json_line(input, state)? else {
            return if expected_method == "thread/unsubscribe" {
                Ok(())
            } else {
                Err(FixtureError::Io)
            };
        };
        let object = request.as_object().ok_or(FixtureError::InvalidArguments)?;
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or(FixtureError::InvalidArguments)?;
        if object.get("id").is_none() && !expected(expected_method, method) {
            continue;
        }
        if !expected(expected_method, method)
            && background::answer(method, &request, output, state)?
        {
            continue;
        }
        if method == "model/list" {
            if request
                .pointer("/params/cursor")
                .is_some_and(|cursor| !cursor.is_null())
            {
                return Err(FixtureError::InvalidArguments);
            }
            if optional_models {
                state.last_request_id = Some(
                    object
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or(FixtureError::InvalidArguments)?,
                );
                emit_response("models", state, output)?;
                optional_models = false;
                continue;
            }
        }
        if !expected(expected_method, method) {
            return Err(FixtureError::InvalidArguments);
        }
        if let Some(id) = object.get("id").and_then(Value::as_u64) {
            state.last_request_id = Some(id);
            if matches!(method, "thread/start" | "thread/resume") {
                state.codex_thread_model = request
                    .pointer("/params/model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            if method == "turn/start" {
                state.codex_turn_config = Some(super::codex_catalog::turn_config(&request));
                state.prompt_echo = request
                    .pointer("/params/input")
                    .and_then(Value::as_array)
                    .map(|items| super::prompt_echo::blocks(items));
            }
            return Ok(());
        }
        if expected_method == "initialized" {
            return Ok(());
        }
        return Err(FixtureError::InvalidArguments);
    }
    Err(FixtureError::ExpandedLimit)
}

/// `expected` names one method, or alternatives separated by `|`.
fn expected(expected: &str, method: &str) -> bool {
    expected.split('|').any(|candidate| candidate == method)
}

pub(super) fn emit_response(
    shape: &str,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let Some(id) = state.last_request_id.take() else {
        return if shape == "ack" {
            Ok(())
        } else {
            Err(FixtureError::InvalidArguments)
        };
    };
    if shape == "goalMethodNotFound" {
        return emit_json(
            output,
            state,
            &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
        );
    }
    let result = match shape {
        "initialize" | "initializeObserved" => json!({
            "userAgent": USER_AGENT,
            "codexHome": if shape == "initializeObserved" {
                std::env::temp_dir().to_string_lossy().into_owned()
            } else {
                std::env::var("CODEX_HOME").map_err(|_| FixtureError::InvalidArguments)?
            },
            "platformFamily": "windows",
            "platformOs": "windows"
        }),
        "thread" => json!({"thread":{"id": THREAD_ID}}),
        "effortThread" => json!({
            "thread":{"id": THREAD_ID},
            "model":"fixture-model-fast","reasoningEffort":"high"
        }),
        "configThread" => super::codex_catalog::config_thread(state, THREAD_ID),
        "models" | "effortModels" | "configModels" => super::codex_catalog::models(shape),
        "turn" => {
            state.turn_count = state
                .turn_count
                .checked_add(1)
                .ok_or(FixtureError::ExpandedLimit)?;
            let turn_id = format!("fixture-turn-{}-{}", std::process::id(), state.turn_count);
            state.current_turn_id = Some(turn_id.clone());
            json!({"turn":{"id":turn_id}})
        }
        "ack" => json!({}),
        "steer" => {
            json!({"turnId": state.current_turn_id.as_deref().ok_or(FixtureError::InvalidArguments)?})
        }
        "fork" => json!({"thread":{"id": FORK_THREAD_ID, "forkedFromId": THREAD_ID}}),
        _ => return Err(FixtureError::InvalidArguments),
    };
    emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","id": id, "result": result}),
    )
}

pub(super) fn emit_notification(
    shape: &str,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let turn_id = state
        .current_turn_id
        .as_deref()
        .ok_or(FixtureError::InvalidArguments)?;
    let notification = match shape {
        "started" => json!({
            "jsonrpc":"2.0",
            "method":"turn/started",
            "params":{"threadId":THREAD_ID,"turn":{"id":turn_id,"status":"inProgress"}}
        }),
        "delta" => json!({
            "jsonrpc":"2.0",
            "method":"item/agentMessage/delta",
            "params":{"threadId":THREAD_ID,"turnId":turn_id,"itemId":format!("fixture-message-{turn_id}"),"delta":"ok"}
        }),
        "itemCompleted" => json!({
            "jsonrpc":"2.0",
            "method":"item/completed",
            "params":{"threadId":THREAD_ID,"turnId":turn_id,"item":{"type":"commandExecution","id":"fixture-command-completed","status":"completed","command":"echo fixture-command-completed","aggregatedOutput":"fixture command completed"}}
        }),
        "completed" => json!({
            "jsonrpc":"2.0",
            "method":"turn/completed",
            "params":{"threadId":THREAD_ID,"turn":{"id":turn_id,"status":"completed"}}
        }),
        "error" => {
            json!({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":THREAD_ID,"turn":{"id":turn_id,"status":"failed","error":{"message":"Synthetic replay failure; retry is safe.","codexErrorInfo":"other","additionalDetails":null}}}})
        }
        "interrupted" => json!({
            "jsonrpc":"2.0",
            "method":"turn/completed",
            "params":{"threadId":THREAD_ID,"turn":{"id":turn_id,"status":"interrupted"}}
        }),
        _ => return Err(FixtureError::InvalidArguments),
    };
    emit_json(output, state, &notification)
}

/// Most messages one `codexTranscript` step may write: enough to fill more
/// than one history page (256 Items) without flooding the driver.
pub(super) const TRANSCRIPT_MESSAGES: u64 = 600;

/// Writes `count` completed assistant messages into the current turn, so a
/// journey can open on a long transcript before it streams.
pub(super) fn emit_transcript(
    count: u64,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if count == 0 || count > TRANSCRIPT_MESSAGES {
        return Err(FixtureError::InvalidArguments);
    }
    let turn_id = state
        .current_turn_id
        .clone()
        .ok_or(FixtureError::InvalidArguments)?;
    for index in 1..=count {
        let item = json!({
            "type": "agentMessage",
            "id": format!("fixture-transcript-{index}"),
            "text": format!(
                "Earlier message {index} of {count}. It is synthetic history, there so the chat has more than one page to scroll back through."
            ),
        });
        emit_json(
            output,
            state,
            &json!({"jsonrpc":"2.0","method":"item/completed","params":{"threadId":THREAD_ID,"turnId":turn_id,"item":item}}),
        )?;
    }
    Ok(())
}

pub(super) fn read_json_line(
    input: &mut impl Read,
    state: &WireState,
) -> Result<Option<Value>, FixtureError> {
    if !state.stdin_open {
        return Err(FixtureError::InvalidArguments);
    }
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = input.read(&mut byte).map_err(|_| FixtureError::Io)?;
        if read == 0 {
            return Ok(None);
        }
        if byte[0] == b'\n' {
            return serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| FixtureError::InvalidArguments);
        }
        if bytes.len() as u64 >= LIMITS.wire.frame_bytes {
            return Err(FixtureError::ExpandedLimit);
        }
        bytes.push(byte[0]);
    }
}

pub(super) fn emit_json(
    output: &mut impl Write,
    state: &mut WireState,
    value: &Value,
) -> Result<(), FixtureError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| FixtureError::InvalidArguments)?;
    bytes.push(b'\n');
    write_stdout(output, state, &bytes, false)?;
    output.flush().map_err(|_| FixtureError::Io)
}

/// Emits one of the Codex frame shapes a journey step names.
pub(super) fn emit_known(
    shape: &str,
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if shape == "codexNative" || shape == "codexNativeThreadless" {
        let mut params = acp::substitute(step["params"].clone(), state, 0)?;
        // A thread-less app-server notice (such as `configWarning`) may
        // arrive before any thread or Turn exists, so it names neither.
        if shape == "codexNative" {
            params["threadId"] = json!(acp::SESSION);
            params["turnId"] = json!(
                state
                    .current_turn_id
                    .as_deref()
                    .ok_or(FixtureError::InvalidArguments)?
            );
        }
        return emit_json(
            output,
            state,
            &json!({"jsonrpc":"2.0","method":field_str(step, "method")?,"params":params}),
        );
    }
    match shape {
        "codexError" => emit_notification("error", state, output),
        "codexInitializeResult" => emit_response("initialize", state, output),
        // The install's skills, from its start (see `background`).
        "codexInitializeResultObserved" => {
            background::install(step, state, true)?;
            emit_response("initializeObserved", state, output)
        }
        "codexSkillsChanged" => background::skills_changed(step, state, output),
        "codexThreadResult" => emit_response("thread", state, output),
        "codexThreadEffortResult" => emit_response("effortThread", state, output),
        "codexModelListResult" => emit_response("models", state, output),
        "codexModelListEffortResult" => emit_response("effortModels", state, output),
        "codexThreadConfigResult" => emit_response("configThread", state, output),
        "codexModelListConfigResult" => emit_response("configModels", state, output),
        "codexGoalMethodNotFound" => emit_response("goalMethodNotFound", state, output),
        "codexTurnResult" => emit_response("turn", state, output),
        "codexSteerResult" => emit_response("steer", state, output),
        "codexForkResult" => emit_response("fork", state, output),
        "codexStarted" => emit_notification("started", state, output),
        "codexAck" => emit_response("ack", state, output),
        "codexDelta" => emit_notification("delta", state, output),
        "codexItemCompleted" => emit_notification("itemCompleted", state, output),
        "codexCompleted" => emit_notification("completed", state, output),
        "codexInterrupted" => emit_notification("interrupted", state, output),
        "codexUserInputRequest" => user_input::emit_request(step, state, output),
        "codexUserInputEcho" => user_input::emit_echo(state, output),
        "codexTranscript" => emit_transcript(field_u64(step, "count")?, state, output),
        _ => Err(FixtureError::InvalidArguments),
    }
}

#[cfg(test)]
#[path = "wire_codex_journey_tests.rs"]
mod journey_tests;
