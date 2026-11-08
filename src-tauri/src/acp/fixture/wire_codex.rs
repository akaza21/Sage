use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::acp::limits::LIMITS;

use super::super::FixtureError;
use super::super::interpreter::field_str;
use super::{WireState, acp, user_input, write_stdout};

const THREAD_ID: &str = "fixture-persisted-session-1";
/// The restart journey's catalogue: its default model, then a second one.
const CONFIG_MODELS: [&str; 2] = ["fixture-model", "fixture-model-deep"];
/// The session a native capture's fork creates (Codex `thread/fork` per the
/// pinned 0.146 schema; Claude `session/fork` per claude-agent-acp 0.76.0).
pub(super) const FORK_THREAD_ID: &str = "fixture-fork-session-1";
const USER_AGENT: &str = "sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)";
const USER_AGENT_0146: &str = "sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)";

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
        if method == "thread/read" {
            emit_child_read(&request, output, state)?;
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

fn emit_child_read(
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    let id = request
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(FixtureError::InvalidArguments)?;
    let params = request
        .get("params")
        .ok_or(FixtureError::InvalidArguments)?;
    let child = params
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|child| matches!(*child, "fixture-child-1" | "fixture-child-2"))
        .ok_or(FixtureError::InvalidArguments)?;
    if params.get("includeTurns").and_then(Value::as_bool) != Some(true) {
        return Err(FixtureError::InvalidArguments);
    }
    emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","id":id,"result":{"thread":{
            "id":child,"parentThreadId":THREAD_ID,"turns":[{
                "id":"fixture-child-turn","itemsView":"full","status":"completed",
                "items":[{"id":"fixture-child-answer","type":"agentMessage","text":"Synthetic child summary"}]
            }]
        }}}),
    )
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
        "initialize" | "initialize0146" => json!({
            "userAgent": if shape == "initialize0146" { USER_AGENT_0146 } else { USER_AGENT },
            "codexHome": if shape == "initialize0146" {
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
        // A restart journey's thread runs the model it was opened or resumed
        // with, at the catalogue's default effort, as Codex reports it.
        "configThread" => json!({
            "thread":{"id": THREAD_ID},
            "model": state.codex_thread_model.as_deref()
                .filter(|model| CONFIG_MODELS.contains(model)).unwrap_or(CONFIG_MODELS[0]),
            "reasoningEffort":"medium"
        }),
        "models" | "effortModels" | "configModels" => {
            let mut catalog = json!({"data":[{
            "id":"fixture-model","model":"fixture-model","displayName":"Synthetic replay",
            "description":"Synthetic native replay model","isDefault":true,"hidden":false,
            "supportedReasoningEfforts":[]
        }],"nextCursor":null});
            if shape != "models" {
                catalog["data"][0]["defaultReasoningEffort"] = json!("medium");
                catalog["data"][0]["supportedReasoningEfforts"] = json!([
                    {"reasoningEffort":"low","description":"Quick synthetic replay"},
                    {"reasoningEffort":"medium","description":"Balanced synthetic replay"},
                    {"reasoningEffort":"high","description":"Thorough synthetic replay"}
                ]);
                let mut alternate = catalog["data"][0].clone();
                alternate["id"] = json!("fixture-model-fast");
                alternate["model"] = json!("fixture-model-fast");
                alternate["displayName"] = json!("Synthetic replay fast");
                alternate["isDefault"] = json!(false);
                alternate["defaultReasoningEffort"] = json!("low");
                if shape == "configModels" {
                    alternate["id"] = json!(CONFIG_MODELS[1]);
                    alternate["model"] = json!(CONFIG_MODELS[1]);
                    alternate["displayName"] = json!("Synthetic replay deep");
                    alternate["defaultReasoningEffort"] = json!("medium");
                }
                catalog["data"].as_array_mut().unwrap().push(alternate);
            }
            catalog
        }
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
    if shape == "codexNative" {
        let mut params = acp::substitute(step["params"].clone(), state, 0)?;
        params["threadId"] = json!(acp::SESSION);
        params["turnId"] = json!(
            state
                .current_turn_id
                .as_deref()
                .ok_or(FixtureError::InvalidArguments)?
        );
        return emit_json(
            output,
            state,
            &json!({"jsonrpc":"2.0","method":field_str(step, "method")?,"params":params}),
        );
    }
    match shape {
        "codexError" => emit_notification("error", state, output),
        "codexInitializeResult" => emit_response("initialize", state, output),
        "codexInitializeResult0146" => emit_response("initialize0146", state, output),
        "codexThreadResult" => emit_response("thread", state, output),
        "codexThreadEffortResult" => emit_response("effortThread", state, output),
        "codexModelListResult" => emit_response("models", state, output),
        "codexModelListEffortResult" => emit_response("effortModels", state, output),
        "codexThreadConfigResult" => emit_response("configThread", state, output),
        "codexModelListConfigResult" => emit_response("configModels", state, output),
        "codexGoalMethodNotFound" => emit_response("goalMethodNotFound", state, output),
        "codexTurnResult" => emit_response("turn", state, output),
        "codexForkResult" => emit_response("fork", state, output),
        "codexStarted" => emit_notification("started", state, output),
        "codexAck" => emit_response("ack", state, output),
        "codexDelta" => emit_notification("delta", state, output),
        "codexItemCompleted" => emit_notification("itemCompleted", state, output),
        "codexCompleted" => emit_notification("completed", state, output),
        "codexInterrupted" => emit_notification("interrupted", state, output),
        "codexUserInputRequest" => user_input::emit_request(step, state, output),
        "codexUserInputEcho" => user_input::emit_echo(state, output),
        _ => Err(FixtureError::InvalidArguments),
    }
}

#[cfg(test)]
mod journey_tests {
    use super::*;

    #[test]
    fn lost_steer_fixture_answers_goal_hydration_without_ending_the_session() {
        let case = super::super::super::generated_cases::JOURNEY_CASES
            .iter()
            .find(|case| case.id == "codex-steer-lost-result")
            .unwrap();
        let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
        let request = br#"{"jsonrpc":"2.0","id":4,"method":"thread/goal/get","params":{}}
"#;
        let mut input = std::io::Cursor::new(request);
        let mut output = Vec::new();
        let mut error = Vec::new();
        let mut state = super::super::WireState::new();
        super::super::execute_with_io(&steps[4], &mut state, &mut input, &mut output, &mut error)
            .unwrap();
        super::super::execute_with_io(&steps[5], &mut state, &mut input, &mut output, &mut error)
            .unwrap();
        assert_eq!(input.position(), request.len() as u64);
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap(),
            json!({"jsonrpc":"2.0","id":4,"error":{"code":-32601,"message":"Method not found"}})
        );
        assert!(error.is_empty());
        assert_eq!(
            steps[6],
            json!({"op":"awaitClient","kind":"codexTurnStart"})
        );
    }

    #[test]
    fn lost_steer_result_consumes_the_correlated_write_without_emitting_a_reply() {
        let case = super::super::super::generated_cases::JOURNEY_CASES
            .iter()
            .find(|case| case.id == "codex-steer-lost-result")
            .unwrap();
        let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
        let request = br#"{"jsonrpc":"2.0","id":4,"method":"turn/steer","params":{}}
"#;
        let mut input = std::io::Cursor::new(request);
        let mut output = Vec::new();
        let mut error = Vec::new();
        let mut state = super::super::WireState::new();
        super::super::execute_with_io(&steps[9], &mut state, &mut input, &mut output, &mut error)
            .unwrap();
        assert_eq!(input.position(), request.len() as u64);
        assert!(output.is_empty());
        assert_eq!(state.last_request_id, Some(4));
        assert_eq!(steps[10], json!({"op":"exit","code":17}));
    }
}
