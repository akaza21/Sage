use std::io::{self, Read, Write};

use serde_json::Value;

use sage_core::limits::LIMITS;

use super::FixtureError;
use super::interpreter::{field_str, field_u64};

#[cfg(test)]
#[path = "wire_acp_tests.rs"]
mod acp_tests;

#[cfg(test)]
#[path = "wire_codex_tests.rs"]
mod codex_tests;
#[cfg(test)]
#[path = "wire_pi_tests.rs"]
mod pi_tests;

#[cfg(test)]
#[path = "wire_child_opened_tests.rs"]
mod child_opened_tests;
#[cfg(test)]
#[path = "wire_parity_tests.rs"]
mod parity_tests;
#[cfg(test)]
pub(crate) use child_opened_tests::child_opened_wire_for_test;
#[cfg(test)]
#[path = "wire_claude_sdk_replay.rs"]
mod claude_sdk_replay;
#[cfg(test)]
pub(crate) use claude_sdk_replay::claude_sdk_wire_for_test;
#[cfg(test)]
pub(crate) use pi_bridge::{pi_journey_steps_for_test, run_pi_step_for_test};
#[cfg(test)]
pub(crate) use user_input::codex_user_input_wire_for_test;

#[path = "wire_acp.rs"]
mod acp;
#[path = "wire_codex_background.rs"]
mod background;
#[path = "wire_claude_sdk.rs"]
mod claude_sdk;
#[path = "wire_claude_sdk_transcript.rs"]
mod claude_transcript;
#[path = "wire_codex.rs"]
mod codex;
#[path = "wire_codex_catalog.rs"]
mod codex_catalog;
#[path = "wire_pi.rs"]
mod pi;
#[path = "wire_pi_bridge.rs"]
mod pi_bridge;
#[path = "wire_prompt_echo.rs"]
mod prompt_echo;
#[path = "wire_codex_user_input.rs"]
mod user_input;

pub(super) struct WireState {
    stdin_open: bool,
    stdout_open: bool,
    expanded_bytes: u64,
    last_request_id: Option<u64>,
    prompt_request_id: Option<u64>,
    current_turn_id: Option<String>,
    turn_count: u64,
    /// Server requests the fixture has sent. Codex numbers its own requests
    /// from one counter per connection, so an id is never reused.
    server_requests_sent: u64,
    pending_user_input: Option<u64>,
    user_input_echo: Option<String>,
    pi: pi::PiWireState,
    acp_config: Option<acp::AcpConfig>,
    embedded_context: Option<acp::EmbeddedContext>,
    /// The model the client opened or resumed the Codex thread with.
    codex_thread_model: Option<String>,
    /// The model and effort the client's last Codex `turn/start` named, as
    /// a frame quotes them (`$CODEX_TURN_CONFIG`).
    codex_turn_config: Option<String>,
    /// The client's last prompt, as a frame quotes it (`$PROMPT_ECHO`).
    prompt_echo: Option<String>,
    claude_sdk: claude_sdk::SdkWireState,
    codex_skills: background::Skills,
}

impl WireState {
    pub(super) fn new() -> Self {
        Self {
            stdin_open: true,
            stdout_open: true,
            expanded_bytes: 0,
            last_request_id: None,
            prompt_request_id: None,
            current_turn_id: None,
            turn_count: 0,
            server_requests_sent: 0,
            pending_user_input: None,
            user_input_echo: None,
            pi: pi::PiWireState::new(),
            acp_config: None,
            embedded_context: None,
            codex_thread_model: None,
            codex_turn_config: None,
            prompt_echo: None,
            claude_sdk: claude_sdk::SdkWireState::default(),
            codex_skills: background::Skills::default(),
        }
    }

    pub(super) fn launched(harness_args: &[String]) -> Result<Self, FixtureError> {
        let mut state = Self::new();
        state.claude_sdk = claude_sdk::SdkWireState::launched(harness_args)?;
        Ok(state)
    }

    fn charge(&mut self, bytes: u64) -> Result<(), FixtureError> {
        self.expanded_bytes = self
            .expanded_bytes
            .checked_add(bytes)
            .ok_or(FixtureError::ExpandedLimit)?;
        if self.expanded_bytes > LIMITS.fixture.expanded_bytes_per_case {
            return Err(FixtureError::ExpandedLimit);
        }
        Ok(())
    }
}

pub(super) fn execute(step: &Value, _seed: u32, state: &mut WireState) -> Result<(), FixtureError> {
    if field_str(step, "op")? == "closeNativeStdin" {
        return close_native_stdin(state);
    }
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut error = io::stderr().lock();
    execute_with_io(step, state, &mut input, &mut output, &mut error)
}

fn execute_with_io(
    step: &Value,
    state: &mut WireState,
    input: &mut impl Read,
    output: &mut impl Write,
    error: &mut impl Write,
) -> Result<(), FixtureError> {
    match field_str(step, "op")? {
        "awaitClient" => await_client_kind(step, input, output, state)?,
        "readPromptBytes" | "readRequestBytes" => {
            await_client(input, state, field_u64(step, "bytes")?)?
        }
        "closeStdin" => state.stdin_open = false,
        "closeStdout" => state.stdout_open = false,
        "emitKnownFrame" => emit_known(step, state, output)?,
        "emitMalformed" => emit_malformed(field_str(step, "shape")?, state, output)?,
        "emitSizedFrame" => emit_sized_frame(field_u64(step, "bytes")?, state, output)?,
        "emitBoundedJson" => super::wire_payload::emit_bounded_json(step, state, output)?,
        "emitStderrLine" => emit_stderr_line(field_u64(step, "bytes")?, state, error)?,
        "fillStderrRing" => fill_stderr(step, state, error)?,
        "emitExternalId" | "emitPermissionPayload" => super::wire_payload::emit_sized_string(
            field_str(step, "op")?,
            field_u64(step, "bytes")?,
            state,
            output,
        )?,
        "emitRpcMethod" => {
            super::wire_payload::emit_sized_method(field_u64(step, "bytes")?, state, output)?
        }
        "emitCapabilityToken" => super::wire_payload::emit_capability_token(step, state, output)?,
        "emitMetadata" => super::wire_payload::emit_metadata(step, state, output)?,
        "emitModels" => super::wire_payload::emit_models(step, state, output)?,
        _ => return Err(FixtureError::UnknownStep),
    }
    Ok(())
}

fn await_client_kind(
    step: &Value,
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    let kind = field_str(step, "kind")?;
    if kind == "piBridgeTool" {
        return pi_bridge::tool(step, input, output, state);
    }
    if kind.starts_with("claudeSdk") {
        return claude_sdk::await_request(input, output, state, step);
    }
    if kind.starts_with("pi") {
        return pi::await_request(input, output, state, step);
    }
    if kind.starts_with("acp") {
        let prompt_text = step.get("promptText").and_then(Value::as_str);
        return acp::await_request(input, output, state, kind, prompt_text);
    }
    if kind == "codexUserInputAnswer" {
        return user_input::await_answer(input, state);
    }
    if kind == "codexGoalGetWithOptionalModelList" {
        return codex::await_goal_with_optional_models(input, output, state);
    }
    let expected_method = match kind {
        "codexInitialize" => Some("initialize"),
        "codexStart" => Some("thread/start"),
        "codexStartOrResume" => Some("thread/start|thread/resume"),
        "codexResume" => Some("thread/resume"),
        "codexModelList" => Some("model/list"),
        "codexGoalGet" => Some("thread/goal/get"),
        "codexTurnStart" => Some("turn/start"),
        "codexSteer" => Some("turn/steer"),
        "codexFork" => Some("thread/fork"),
        "codexInterrupt" => Some("turn/interrupt"),
        "codexUnsubscribe" => Some("thread/unsubscribe"),
        _ => None,
    };
    match expected_method {
        Some(method) => {
            codex::await_request(input, output, state, method)?;
            if method == "turn/interrupt" {
                let _ = codex::emit_response("ack", state, output);
                codex::emit_notification("interrupted", state, output)?;
            }
            Ok(())
        }
        None => await_client(input, state, LIMITS.wire.frame_bytes),
    }
}

fn await_client(
    input: &mut impl Read,
    state: &mut WireState,
    maximum: u64,
) -> Result<(), FixtureError> {
    if !state.stdin_open || maximum > LIMITS.wire.frame_bytes {
        return Err(FixtureError::InvalidArguments);
    }
    let mut observed = 0_u64;
    let mut byte = [0_u8; 1];
    loop {
        let read = input.read(&mut byte).map_err(|_| FixtureError::Io)?;
        if read == 0 || byte[0] == b'\n' {
            return Ok(());
        }
        observed = observed.checked_add(1).ok_or(FixtureError::ExpandedLimit)?;
        if observed > maximum {
            return Err(FixtureError::ExpandedLimit);
        }
    }
}

#[cfg(any(test, all(windows, debug_assertions)))]
pub(super) fn codex_reply(shape: &str, id: u64) -> Option<Value> {
    let mut state = WireState::new();
    state.last_request_id = Some(id);
    let mut output = Vec::new();
    codex::emit_known(shape, &Value::Null, &mut state, &mut output).ok()?;
    serde_json::from_slice(&output).ok()
}

fn emit_known(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let shape = field_str(step, "shape")?;
    if shape == "piNative" {
        return pi::emit(step, state, output);
    }
    if matches!(shape, "claudeSdkNative" | "claudeSdkLifecycle") {
        return claude_sdk::emit(step, state, output);
    }
    if shape.starts_with("acp") {
        return acp::emit(step, state, output);
    }
    if shape.starts_with("codex") {
        return codex::emit_known(shape, step, state, output);
    }
    match shape {
        "childActivityDepth4"
        | "childActivityDepth5"
        | "childActivityForeignParent"
        | "childActivityText"
        | "initializeSuccess"
        | "newSessionSuccess"
        | "nativeForkSuccess"
        | "parentToolStart"
        | "permissionRequestA"
        | "permissionRequestAConflictingPayload"
        | "resumeFailure"
        | "resumeSuccess"
        | "stdinClosedBarrier"
        | "textComplete"
        | "textDelta"
        | "textStart"
        | "toolCompletedA"
        | "toolFailedA"
        | "turnCancelled"
        | "turnComplete"
        | "unknownBoundedNotification"
        | "unknownCollaborationOperation"
        | "unknownRequest" => {}
        _ => return Err(FixtureError::InvalidArguments),
    }
    let request_id = if shape == "unknownRequest" {
        ",\"id\":1"
    } else {
        ""
    };
    let params = match shape {
        "parentToolStart" => serde_json::json!({
            "externalItemId": field_str(step, "externalItemId")?,
        }),
        "childActivityText" => serde_json::json!({
            "externalActivityId": field_str(step, "externalActivityId")?,
            "parentExternalItemId": field_str(step, "parentExternalItemId")?,
            "text": field_str(step, "text")?,
        }),
        "childActivityForeignParent" => serde_json::json!({
            "externalActivityId": field_str(step, "externalActivityId")?,
            "parentExternalItemId": field_str(step, "parentExternalItemId")?,
        }),
        "resumeSuccess" => serde_json::json!({
            "externalSessionId": field_str(step, "externalSessionId")?,
        }),
        "newSessionSuccess" => serde_json::json!({
            "externalSessionId": field_str(step, "externalSessionId")?,
        }),
        _ => serde_json::json!({}),
    };
    let frame = format!(
        "{{\"jsonrpc\":\"2.0\"{request_id},\"method\":\"fixture/{shape}\",\"params\":{params}}}"
    );
    let delimiter = match field_str(step, "delimiter")? {
        "lf" => b"\n".as_slice(),
        "crlf" => b"\r\n".as_slice(),
        _ => return Err(FixtureError::InvalidArguments),
    };
    let split = field_str(step, "split")?;
    write_stdout(output, state, frame.as_bytes(), split == "everyByte")?;
    write_stdout(output, state, delimiter, split == "everyByte")
}

#[cfg(windows)]
fn close_native_stdin(state: &mut WireState) -> Result<(), FixtureError> {
    use std::os::windows::io::AsRawHandle;

    let stdin = io::stdin();
    let result =
        unsafe { windows_sys::Win32::Foundation::CloseHandle(stdin.as_raw_handle().cast()) };
    if result == 0 {
        return Err(FixtureError::Io);
    }
    state.stdin_open = false;
    Ok(())
}

#[cfg(unix)]
fn close_native_stdin(state: &mut WireState) -> Result<(), FixtureError> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let stdin = io::stdin();
    drop(unsafe { OwnedFd::from_raw_fd(stdin.as_raw_fd()) });
    state.stdin_open = false;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn close_native_stdin(_state: &mut WireState) -> Result<(), FixtureError> {
    Err(FixtureError::HostOracleRequired)
}

fn emit_malformed(
    shape: &str,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let bytes: &[u8] = match shape {
        "invalidUtf8" => &[b'{', b'"', 0xff, b'"', b'}', b'\n'],
        "embeddedNul" => b"{\"x\":\"\0\"}\n",
        "partialJsonWithoutDelimiter" => b"{\"partial\":true}",
        "topLevelArray" => b"[]\n",
        "twoObjectsOneFrame" => b"{}{}\n",
        "duplicateDecodedKey" => b"{\"a\":1,\"\\u0061\":2}\n",
        _ => return Err(FixtureError::InvalidArguments),
    };
    write_stdout(output, state, bytes, false)
}

fn emit_sized_frame(
    bytes: u64,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if !(2..=LIMITS.fixture.expanded_bytes_per_case).contains(&bytes) {
        return Err(FixtureError::ExpandedLimit);
    }
    write_stdout(output, state, b"{", false)?;
    emit_repeated(output, state, b' ', bytes - 2)?;
    write_stdout(output, state, b"}\n", false)
}

#[path = "wire_output.rs"]
mod output;
pub(super) use output::{emit_repeated, write_stdout};
use output::{emit_stderr_line, fill_stderr};

#[cfg(test)]
#[path = "wire_capture_steps.rs"]
mod capture;
#[cfg(test)]
pub(super) use capture::capture_steps;
#[cfg(test)]
pub(crate) use capture::replay_native;
#[cfg(test)]
#[path = "wire_echo_journey_tests.rs"]
mod echo_journey_tests;
