#![cfg(feature = "acp-fixture")]

#[cfg(any(windows, test))]
mod adapter;
mod cases;
#[cfg(all(test, windows))]
mod characterization_external_tests;
#[cfg(test)]
mod child;
#[cfg(test)]
mod coverage;
#[cfg(all(test, windows))]
mod external_delivery_tests;
#[cfg(all(test, windows))]
mod external_fixture_adapters;
#[cfg(all(test, windows))]
mod external_fixture_runtime;
#[cfg(all(test, windows))]
mod external_fixture_support;
#[cfg(all(test, windows))]
mod external_tests;
mod generated_cases;
#[cfg(test)]
mod host;
mod interpreter;
#[cfg(test)]
mod ledger_limit_tests;
#[cfg(test)]
mod oracle;
mod process_ops;
#[cfg(all(test, windows))]
mod resume_external_tests;
#[cfg(test)]
mod test_support;
#[cfg(all(test, windows))]
mod windows_literal_tests;
mod wire;
mod wire_payload;

use std::ffi::OsString;

use self::cases::FixtureInvocation;

#[cfg(windows)]
pub(crate) use adapter::FixtureAdapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureError {
    DeferredCase,
    ExpandedLimit,
    HostOracleRequired,
    InstructionLimit,
    InvalidArguments,
    Io,
    UnknownStep,
}

pub fn is_native_replay_case(case: &str) -> bool {
    generated_cases::JOURNEY_CASES
        .iter()
        .any(|entry| entry.id == case)
}

#[cfg(test)]
pub(crate) use wire::{
    child_opened_wire_for_test, claude_sdk_wire_for_test, codex_user_input_wire_for_test,
    pi_bridge_calls_for_test, replay_native, run_pi_bridge_call_for_test,
};

#[cfg(all(windows, debug_assertions))]
pub(crate) fn native_case_lists_models(case: &str) -> bool {
    matches!(case, "codex-effort-menu" | "codex-model-menu")
}

#[cfg(all(windows, debug_assertions))]
pub(crate) fn native_case_matches(case: &str, protocol: &str) -> bool {
    generated_cases::JOURNEY_CASES
        .iter()
        .any(|entry| entry.id == case && entry.id.starts_with(&format!("{protocol}-")))
}

/// A Pi replay case that drives tool calls through the bundled Sage bridge
/// extension, so its child stands in for a Pi with that extension installed.
#[cfg(all(windows, debug_assertions))]
pub(crate) fn native_case_uses_pi_bridge(case: &str) -> bool {
    generated_cases::JOURNEY_CASES
        .iter()
        .filter(|entry| entry.id == case && entry.id.starts_with("pi-"))
        .filter_map(|entry| serde_json::from_str::<Vec<serde_json::Value>>(entry.steps_json).ok())
        .any(|steps| {
            steps
                .iter()
                .any(|step| step["op"] == "awaitClient" && step["kind"] == "piBridgeTool")
        })
}

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<i32, FixtureError> {
    interpreter::execute(FixtureInvocation::parse(arguments)?)
}

#[cfg(test)]
mod tests;
