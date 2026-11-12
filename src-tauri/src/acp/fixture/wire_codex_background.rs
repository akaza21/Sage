//! Requests Sage sends a Codex replay outside the scripted steps: a child
//! thread's read and the skills read while a session is idle.

use std::io::Write;

use serde_json::{Value, json};

use super::super::FixtureError;
use super::WireState;
use super::codex::{THREAD_ID, emit_json};

/// Answers `request` when it is a background read, and says whether it was.
pub(super) fn answer(
    method: &str,
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<bool, FixtureError> {
    match method {
        "thread/read" => child_read(request, output, state)?,
        // A replay case has no skills, as a Codex install with none answers.
        "skills/list" => {
            let id = request_id(request)?;
            emit_json(
                output,
                state,
                &json!({"jsonrpc":"2.0","id":id,"result":{"data":[]}}),
            )?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn request_id(request: &Value) -> Result<u64, FixtureError> {
    request
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(FixtureError::InvalidArguments)
}

fn child_read(
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    let id = request_id(request)?;
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
