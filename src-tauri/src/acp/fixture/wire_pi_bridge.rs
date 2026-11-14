//! Replays one Pi tool call through the bundled Sage bridge extension
//! (`bindings/pi/bridge/extension.mjs`) without a real Pi. The step emits the
//! extension's own dialog requests and blocks on Sage's replies exactly where
//! `execute()` awaits `ctx.ui.input`, so approval and the workspace write run
//! through Sage's real permission card and bridge write path.

use std::io::{Read, Write};

use serde_json::{Value, json};

use super::super::FixtureError;
use super::field_str;
use super::{WireState, acp, codex};
use crate::acp::bindings::pi::bridge::MAX_REPLY_BYTES;

/// The extension's request-size ceiling (`MAX_BYTES` in extension.mjs).
const MAX_PREPARE_BYTES: usize = 262_144;

pub(super) fn tool(
    step: &Value,
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    let tool_call_id = bounded_id(field_str(step, "toolCallId")?)?;
    let tool = field_str(step, "tool")?;
    if !matches!(tool, "write" | "edit") {
        return Err(FixtureError::InvalidArguments);
    }
    let args = acp::substitute(step["args"].clone(), state, 0)?;
    if !args.is_object() {
        return Err(FixtureError::InvalidArguments);
    }
    let prepare_id = bounded_id(field_str(step, "prepareId")?)?;
    let commit_id = bounded_id(field_str(step, "commitId")?)?;
    // A replay re-sends the extension request for a call Pi already ran; Pi
    // reports no new tool lifecycle for it, and Sage must refuse it.
    let replay = step.get("replay").and_then(Value::as_bool).unwrap_or(false);
    let expected_refusal = step.get("expectRefusal").and_then(Value::as_str);
    if !replay {
        codex::emit_json(
            output,
            state,
            &json!({"type":"tool_execution_start","toolCallId":tool_call_id,"toolName":tool,"args":args}),
        )?;
    }
    let request = serde_json::to_string(
        &json!({"version":1,"toolCallId":tool_call_id,"tool":tool,"args":args}),
    )
    .map_err(|_| FixtureError::InvalidArguments)?;
    if request.len() > MAX_PREPARE_BYTES {
        return Err(FixtureError::ExpandedLimit);
    }
    let prepared = dialog(
        input,
        output,
        state,
        prepare_id,
        "sage.pi.prepare.v1",
        &request,
    )?;
    // The extension raises every answer but "approved" / "written" as the
    // tool's error, carrying Sage's reason.
    let approved = prepared == "approved";
    let answer = if approved {
        dialog(
            input,
            output,
            state,
            commit_id,
            "sage.pi.commit.v1",
            tool_call_id,
        )?
    } else {
        prepared
    };
    if let Some(expected) = expected_refusal
        && answer != expected
    {
        return Err(FixtureError::InvalidArguments);
    }
    let written = answer == "written";
    if replay {
        // Sage accepting a replayed request is the failure this step exists to catch.
        return if approved {
            Err(FixtureError::InvalidArguments)
        } else {
            Ok(())
        };
    }
    let text = if written {
        "Workspace file updated by Sage."
    } else if answer.is_empty() {
        "Sage did not answer the approval request"
    } else {
        answer.as_str()
    };
    codex::emit_json(
        output,
        state,
        &json!({"type":"tool_execution_end","toolCallId":tool_call_id,"toolName":tool,"result":{"content":[{"type":"text","text":text}],"details":{}},"isError":!written}),
    )
}

/// Emits one `ctx.ui.input` request and returns Sage's answer; a cancelled
/// dialog answers nothing, which the extension treats as a refusal.
fn dialog(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    id: &str,
    title: &str,
    placeholder: &str,
) -> Result<String, FixtureError> {
    codex::emit_json(
        output,
        state,
        &json!({"type":"extension_ui_request","id":id,"method":"input","title":title,"placeholder":placeholder}),
    )?;
    let reply = codex::read_json_line(input, state)?.ok_or(FixtureError::Io)?;
    if reply["type"] != "extension_ui_response" || reply["id"] != id {
        return Err(FixtureError::InvalidArguments);
    }
    if reply["cancelled"] == true {
        return Ok(String::new());
    }
    reply["value"]
        .as_str()
        .filter(|value| value.len() <= MAX_REPLY_BYTES)
        .map(str::to_owned)
        .ok_or(FixtureError::InvalidArguments)
}

fn bounded_id(id: &str) -> Result<&str, FixtureError> {
    (!id.is_empty() && id.len() <= 500 && !id.contains('\0'))
        .then_some(id)
        .ok_or(FixtureError::InvalidArguments)
}

/// The generated Pi journey's steps, in replay order.
#[cfg(test)]
pub(crate) fn pi_journey_steps_for_test(case: &str) -> Vec<Value> {
    let case = crate::acp_protocol::native_cases::JOURNEY_CASES
        .iter()
        .find(|entry| entry.id == case)
        .expect("generated Pi case");
    serde_json::from_str(case.steps_json).expect("generated steps")
}

/// Runs one journey step against a caller-owned Sage peer.
#[cfg(test)]
pub(crate) fn run_pi_step_for_test(step: &Value, input: &mut impl Read, output: &mut impl Write) {
    super::execute_with_io(step, &mut WireState::new(), input, output, &mut Vec::new())
        .expect("step replays");
}
