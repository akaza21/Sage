use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::acp::limits::LIMITS;

use super::super::FixtureError;
use super::{WireState, write_stdout};

const THREAD_ID: &str = "fixture-persisted-session-1";
const USER_AGENT: &str = "sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)";
const USER_AGENT_0146: &str = "sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)";

pub(super) fn await_request(
    input: &mut impl Read,
    state: &mut WireState,
    expected_method: &str,
) -> Result<(), FixtureError> {
    loop {
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
        if object.get("id").is_none() && expected_method != method {
            continue;
        }
        if method != expected_method {
            return Err(FixtureError::InvalidArguments);
        }
        if let Some(id) = object.get("id").and_then(Value::as_u64) {
            state.last_request_id = Some(id);
            return Ok(());
        }
        if expected_method == "initialized" {
            return Ok(());
        }
        return Err(FixtureError::InvalidArguments);
    }
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
        "interrupted" => json!({
            "jsonrpc":"2.0",
            "method":"turn/completed",
            "params":{"threadId":THREAD_ID,"turn":{"id":turn_id,"status":"interrupted"}}
        }),
        _ => return Err(FixtureError::InvalidArguments),
    };
    emit_json(output, state, &notification)
}

fn read_json_line(input: &mut impl Read, state: &WireState) -> Result<Option<Value>, FixtureError> {
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

fn emit_json(
    output: &mut impl Write,
    state: &mut WireState,
    value: &Value,
) -> Result<(), FixtureError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| FixtureError::InvalidArguments)?;
    bytes.push(b'\n');
    write_stdout(output, state, &bytes, false)?;
    output.flush().map_err(|_| FixtureError::Io)
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
