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
#[cfg(any(test, all(windows, debug_assertions)))]
mod discovery;
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
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) use discovery::native_case_discovery_reply;
#[cfg(all(windows, debug_assertions))]
pub(crate) use discovery::native_case_offers_models;

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
    child_opened_wire_for_test, codex_user_input_wire_for_test, pi_bridge_calls_for_test,
    run_pi_bridge_call_for_test,
};

/// A Codex replay case that answers `model/list`, so Sage reads the model
/// catalogue after the thread opens. Taken from the case's own steps: a case
/// that waits for the request must get it, and one that does not would read
/// it as out of order.
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) fn native_case_lists_models(case: &str) -> bool {
    case.starts_with("codex-") && native_case_awaits(case, &["codexModelList"])
}

/// A replay case that opens a new session on its first launch and resumes the
/// same one after an app restart, so it needs a second launch on its profile.
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) fn native_case_spans_restart(case: &str) -> bool {
    native_case_awaits(case, &["acpOpen", "codexStartOrResume"])
}

#[cfg(any(test, all(windows, debug_assertions)))]
fn native_case_awaits(case: &str, kinds: &[&str]) -> bool {
    generated_cases::JOURNEY_CASES
        .iter()
        .filter(|entry| entry.id == case)
        .filter_map(|entry| serde_json::from_str::<Vec<serde_json::Value>>(entry.steps_json).ok())
        .any(|steps| {
            steps.iter().any(|step| {
                step["op"] == "awaitClient"
                    && step["kind"]
                        .as_str()
                        .is_some_and(|kind| kinds.contains(&kind))
            })
        })
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
    case.starts_with("pi-") && native_case_awaits(case, &["piBridgeTool"])
}

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<i32, FixtureError> {
    interpreter::execute(FixtureInvocation::parse(arguments)?)
}

#[cfg(test)]
mod tests;
