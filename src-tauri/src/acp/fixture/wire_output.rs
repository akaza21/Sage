use super::{FixtureError, WireState};
use crate::acp::fixture::interpreter::field_u64;
use serde_json::Value;
use std::io::Write;

pub(in crate::acp::fixture) fn emit_repeated(
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

pub(in crate::acp::fixture) fn write_stdout(
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

pub(super) fn emit_stderr_line(
    bytes: u64,
    state: &mut WireState,
    error: &mut impl Write,
) -> Result<(), FixtureError> {
    emit_repeated(error, state, b'x', bytes)?;
    write_stderr(error, state, b"\n")
}

pub(super) fn fill_stderr(
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
