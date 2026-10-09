use super::{FixtureError, WireState};
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

pub(super) fn write_stderr(
    error: &mut impl Write,
    state: &mut WireState,
    bytes: &[u8],
) -> Result<(), FixtureError> {
    state.charge(bytes.len() as u64)?;
    error.write_all(bytes).map_err(|_| FixtureError::Io)
}
