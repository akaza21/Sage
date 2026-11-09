//! Claude Code's Agent SDK protocol for native replay cases.
//!
//! The child answers every control request the SDK binding sends (initialize,
//! settings, mode, model, effort, interrupt) and waits for the client frame a
//! step names. Turn output is replayed verbatim from `claudeSdkNative` steps.

use std::io::{Read, Write};

use serde_json::{Value, json};

use super::super::FixtureError;
use super::{WireState, acp, codex};

const MAX_FRAMES_PER_STEP: usize = 32;

pub(super) fn await_request(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    kind: &str,
) -> Result<(), FixtureError> {
    let expected = match kind {
        "claudeSdkInitialize" => "initialize",
        "claudeSdkSettings" => "get_settings",
        "claudeSdkInterrupt" => "interrupt",
        "claudeSdkPrompt" => "user",
        "claudeSdkPermission" => "control_response",
        "claudeSdkIdle" => "idle",
        _ => return Err(FixtureError::InvalidArguments),
    };
    for _ in 0..MAX_FRAMES_PER_STEP {
        let Some(frame) = codex::read_json_line(input, state)? else {
            return if expected == "idle" {
                Ok(())
            } else {
                Err(FixtureError::Io)
            };
        };
        let observed = match frame["type"].as_str() {
            Some("control_request") => answer(&frame, state, output)?,
            Some("user") if frame["message"]["content"].is_array() => "user",
            Some("control_response") if frame["response"]["subtype"] == "success" => {
                "control_response"
            }
            _ => return Err(FixtureError::InvalidArguments),
        };
        if observed == expected {
            return Ok(());
        }
        if observed == "user" || observed == "control_response" {
            return Err(FixtureError::InvalidArguments);
        }
    }
    Err(FixtureError::ExpandedLimit)
}

/// Answers one control request and names its subtype.
fn answer(
    frame: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<&'static str, FixtureError> {
    let request = &frame["request"];
    let (subtype, response) = match request["subtype"].as_str() {
        Some("initialize") => ("initialize", initialize()),
        Some("get_settings") => (
            "get_settings",
            json!({"applied": {"model": "claude-fixture", "effort": "medium"}}),
        ),
        Some("set_model") => ("set_model", json!({"model": request["model"]})),
        Some("set_permission_mode") => ("set_permission_mode", json!({"mode": request["mode"]})),
        Some("apply_flag_settings") => ("apply_flag_settings", json!({})),
        Some("interrupt") => ("interrupt", json!({})),
        _ => return Err(FixtureError::InvalidArguments),
    };
    let request_id = frame["request_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 64)
        .ok_or(FixtureError::InvalidArguments)?;
    codex::emit_json(
        output,
        state,
        &json!({"type": "control_response", "response": {
            "subtype": "success", "request_id": request_id, "response": response,
        }}),
    )?;
    Ok(subtype)
}

fn initialize() -> Value {
    let model = |value: &str, name: &str| {
        json!({
            "value": value, "displayName": name, "description": "Synthetic replay",
            "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high"],
        })
    };
    json!({
        "commands": [{"name": "compact", "description": "Compact the conversation", "argumentHint": "[focus]"}],
        "models": [model("default", "Synthetic replay"), model("fixture-fast", "Synthetic fast")],
        "current_permission_mode": "default",
    })
}

pub(super) fn emit(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let frame = acp::substitute(step["frame"].clone(), state, 0)?;
    if !frame.is_object() || frame["type"].as_str().is_none() {
        return Err(FixtureError::InvalidArguments);
    }
    codex::emit_json(output, state, &frame)
}

/// Replays an SDK journey against a client: before each awaited step the
/// client sees the frames emitted since its last turn and returns the frames
/// it sends; the final call (no kind) receives the remaining output.
#[cfg(test)]
pub(crate) fn claude_sdk_wire_for_test(
    case_id: &str,
    mut client: impl FnMut(Option<&str>, Vec<Value>) -> Vec<Value>,
) {
    let case = super::super::generated_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == case_id)
        .expect("SDK journey exists");
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).expect("journey steps");
    let mut state = WireState::new();
    let mut output = Vec::new();
    let drain = |output: &mut Vec<u8>| -> Vec<Value> {
        String::from_utf8(std::mem::take(output))
            .expect("utf-8 output")
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON frame"))
            .collect()
    };
    for step in &steps {
        let mut input = Vec::new();
        if step["op"] == "awaitClient" {
            for frame in client(step["kind"].as_str(), drain(&mut output)) {
                input.extend(format!("{frame}\n").into_bytes());
            }
        }
        let mut input = std::io::Cursor::new(input);
        super::execute_with_io(step, &mut state, &mut input, &mut output, &mut Vec::new())
            .expect("journey step replays");
        assert_eq!(input.position(), input.get_ref().len() as u64);
    }
    client(None, drain(&mut output));
}
