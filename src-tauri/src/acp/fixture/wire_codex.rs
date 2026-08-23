use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::acp::limits::LIMITS;

use super::super::FixtureError;
use super::{WireState, write_stdout};

const THREAD_ID: &str = "fixture-persisted-session-1";
const USER_AGENT: &str = "sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)";

pub(super) fn await_request(
    input: &mut impl Read,
    state: &mut WireState,
    expected_method: &str,
) -> Result<(), FixtureError> {
    loop {
        let request = read_json_line(input, state)?;
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
    let id = state
        .last_request_id
        .take()
        .ok_or(FixtureError::InvalidArguments)?;
    let result = match shape {
        "initialize" => json!({
            "userAgent": USER_AGENT,
            "codexHome": std::env::var("CODEX_HOME").map_err(|_| FixtureError::InvalidArguments)?,
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

fn read_json_line(input: &mut impl Read, state: &WireState) -> Result<Value, FixtureError> {
    if !state.stdin_open {
        return Err(FixtureError::InvalidArguments);
    }
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = input.read(&mut byte).map_err(|_| FixtureError::Io)?;
        if read == 0 {
            return Err(FixtureError::Io);
        }
        if byte[0] == b'\n' {
            return serde_json::from_slice(&bytes).map_err(|_| FixtureError::InvalidArguments);
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
