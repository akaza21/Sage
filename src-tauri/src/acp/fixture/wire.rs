use std::io::{self, Read, Write};

use serde_json::Value;

use crate::acp::limits::LIMITS;

use super::FixtureError;
use super::interpreter::{field_str, field_u64};

#[path = "wire_codex.rs"]
mod codex;

pub(super) struct WireState {
    stdin_open: bool,
    stdout_open: bool,
    expanded_bytes: u64,
    last_request_id: Option<u64>,
    current_turn_id: Option<String>,
    turn_count: u64,
}

impl WireState {
    pub(super) fn new() -> Self {
        Self {
            stdin_open: true,
            stdout_open: true,
            expanded_bytes: 0,
            last_request_id: None,
            current_turn_id: None,
            turn_count: 0,
        }
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
    let expected_method = match field_str(step, "kind")? {
        "codexInitialize" => Some("initialize"),
        "codexStart" => Some("thread/start"),
        "codexResume" => Some("thread/resume"),
        "codexGoalGet" => Some("thread/goal/get"),
        "codexTurnStart" => Some("turn/start"),
        "codexSteer" => Some("turn/steer"),
        "codexInterrupt" => Some("turn/interrupt"),
        "codexUnsubscribe" => Some("thread/unsubscribe"),
        _ => None,
    };
    match expected_method {
        Some(method) => {
            codex::await_request(input, state, method)?;
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

fn emit_known(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let shape = field_str(step, "shape")?;
    match shape {
        "codexInitializeResult" => return codex::emit_response("initialize", state, output),
        "codexInitializeResult0146" => {
            return codex::emit_response("initialize0146", state, output);
        }
        "codexThreadResult" => return codex::emit_response("thread", state, output),
        "codexGoalMethodNotFound" => {
            return codex::emit_response("goalMethodNotFound", state, output);
        }
        "codexTurnResult" => return codex::emit_response("turn", state, output),
        "codexStarted" => return codex::emit_notification("started", state, output),
        "codexAck" => return codex::emit_response("ack", state, output),
        "codexDelta" => return codex::emit_notification("delta", state, output),
        "codexItemCompleted" => return codex::emit_notification("itemCompleted", state, output),
        "codexCompleted" => return codex::emit_notification("completed", state, output),
        "codexInterrupted" => return codex::emit_notification("interrupted", state, output),
        _ => {}
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

fn emit_stderr_line(
    bytes: u64,
    state: &mut WireState,
    error: &mut impl Write,
) -> Result<(), FixtureError> {
    emit_repeated(error, state, b'x', bytes)?;
    write_stderr(error, state, b"\n")
}

fn fill_stderr(
    step: &Value,
    state: &mut WireState,
    error: &mut impl Write,
) -> Result<(), FixtureError> {
    if let Some(chunks) = step.get("rawChunks").and_then(Value::as_array) {
        for chunk in chunks {
            let bytes = field_u64(chunk, "bytes")?;
            for _ in 0..field_u64(chunk, "count")? {
                emit_repeated(error, state, b'x', bytes)?;
            }
        }
        if let Some(final_bytes) = step.get("finalRawChunkBytes").and_then(Value::as_u64) {
            emit_repeated(error, state, b'x', final_bytes)?;
        }
        return Ok(());
    }
    let repeated = step
        .get("repeatedLineBytes")
        .ok_or(FixtureError::InvalidArguments)?;
    let bytes = field_u64(repeated, "bytes")?;
    let count = field_u64(repeated, "count")?;
    for _ in 0..count {
        emit_stderr_line(bytes, state, error)?;
    }
    if let Some(final_bytes) = step.get("finalLineBytes").and_then(Value::as_u64) {
        emit_stderr_line(final_bytes, state, error)?;
    }
    Ok(())
}

pub(super) fn emit_repeated(
    output: &mut impl Write,
    state: &mut WireState,
    byte: u8,
    bytes: u64,
) -> Result<(), FixtureError> {
    state.charge(bytes)?;
    let chunk = [byte; 4096];
    let mut remaining = usize::try_from(bytes).map_err(|_| FixtureError::ExpandedLimit)?;
    while remaining != 0 {
        let take = remaining.min(chunk.len());
        output
            .write_all(&chunk[..take])
            .map_err(|_| FixtureError::Io)?;
        remaining -= take;
    }
    Ok(())
}

pub(super) fn write_stdout(
    output: &mut impl Write,
    state: &mut WireState,
    bytes: &[u8],
    flush_each: bool,
) -> Result<(), FixtureError> {
    if !state.stdout_open {
        return Err(FixtureError::Io);
    }
    state.charge(bytes.len() as u64)?;
    if flush_each {
        for byte in bytes {
            output.write_all(&[*byte]).map_err(|_| FixtureError::Io)?;
            output.flush().map_err(|_| FixtureError::Io)?;
        }
    } else {
        output.write_all(bytes).map_err(|_| FixtureError::Io)?;
    }
    Ok(())
}

fn write_stderr(
    error: &mut impl Write,
    state: &mut WireState,
    bytes: &[u8],
) -> Result<(), FixtureError> {
    state.charge(bytes.len() as u64)?;
    error.write_all(bytes).map_err(|_| FixtureError::Io)
}

#[cfg(test)]
pub(super) fn capture_steps(steps: &[Value]) -> Result<(Vec<u8>, Vec<u8>), FixtureError> {
    let awaiting = steps
        .iter()
        .filter(|step| {
            matches!(
                step.get("op").and_then(Value::as_str),
                Some("awaitClient" | "readPromptBytes" | "readRequestBytes")
            )
        })
        .count();
    let mut input = io::Cursor::new(vec![b'\n'; awaiting]);
    let mut output = Vec::new();
    let mut error = Vec::new();
    let mut state = WireState::new();
    for step in steps {
        if matches!(
            super::interpreter::classify(field_str(step, "op")?)?,
            super::interpreter::StepOwner::Wire
        ) {
            execute_with_io(step, &mut state, &mut input, &mut output, &mut error)?;
        }
    }
    Ok((output, error))
}
