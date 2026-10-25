use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::acp::limits::LIMITS;

use super::FixtureError;
use super::interpreter::{field_str, field_u64};

#[cfg(test)]
#[path = "pause_tests.rs"]
mod pause_tests;

/// One `pauseMs` step sleeps at most this long; a case's pauses together
/// stay within `fixture_delay_maximum`.
pub(super) const PAUSE_STEP_MAXIMUM_MS: u64 = 1_000;

pub(super) struct ProcessState {
    children: Vec<Child>,
    paused_ms: u64,
}

impl ProcessState {
    pub(super) fn new() -> Self {
        Self {
            children: Vec::with_capacity(
                usize::try_from(LIMITS.process.windows_job_active_process_limit)
                    .unwrap_or_default(),
            ),
            paused_ms: 0,
        }
    }
}

pub(super) fn execute(
    step: &Value,
    _case_id: &str,
    seed: u32,
    exit_code: &mut i32,
    state: &mut ProcessState,
) -> Result<(), FixtureError> {
    match field_str(step, "op")? {
        "delayUntilIntentChanges" => bounded_delay(seed),
        "pauseMs" => pause(field_u64(step, "ms")?, state)?,
        "exit" => {
            *exit_code = i32::try_from(field_u64(step, "code")?)
                .map_err(|_| FixtureError::InvalidArguments)?;
        }
        "spawnFixtureChildren" => spawn_children(field_u64(step, "count")?, seed, state)?,
        "spawnTree" => {
            if field_u64(step, "roots")? != 1 {
                return Err(FixtureError::InvalidArguments);
            }
            spawn_children(field_u64(step, "descendants")?, seed, state)?;
        }
        // These race/failpoint operations are executed by the native host test;
        // reaching them in the child is explicit rather than a silent no-op.
        "launchShape"
        | "openVerifiedArtifact"
        | "replaceArtifactPath"
        | "spawnFromVerifiedHandle" => return Err(FixtureError::HostOracleRequired),
        _ => return Err(FixtureError::UnknownStep),
    }
    Ok(())
}

fn bounded_delay(seed: u32) {
    let maximum = u32::try_from(LIMITS.deadlines_ms.fixture_delay_maximum).unwrap_or(u32::MAX);
    std::thread::sleep(Duration::from_millis(u64::from(seed.min(maximum))));
}

/// Paces a replay (word-level stream chunks): the frames already written
/// reach the reader before the bounded sleep.
fn pause(ms: u64, state: &mut ProcessState) -> Result<(), FixtureError> {
    if ms == 0 || ms > PAUSE_STEP_MAXIMUM_MS {
        return Err(FixtureError::InvalidArguments);
    }
    let total = state.paused_ms.saturating_add(ms);
    if total > LIMITS.deadlines_ms.fixture_delay_maximum {
        return Err(FixtureError::InstructionLimit);
    }
    state.paused_ms = total;
    std::io::stdout().flush().map_err(|_| FixtureError::Io)?;
    std::thread::sleep(Duration::from_millis(ms));
    Ok(())
}

fn spawn_children(count: u64, seed: u32, state: &mut ProcessState) -> Result<(), FixtureError> {
    if count > LIMITS.process.windows_job_active_process_limit {
        return Err(FixtureError::ExpandedLimit);
    }
    let executable = std::env::current_exe().map_err(|_| FixtureError::Io)?;
    let seed = seed.to_string();
    for _ in 0..count {
        let child = Command::new(&executable)
            .args([
                "--case",
                "stale-project-generation-completion",
                "--seed",
                &seed,
            ])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| FixtureError::Io)?;
        state.children.push(child);
    }
    Ok(())
}
