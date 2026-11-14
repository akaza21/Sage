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
    crate::acp_protocol::native_cases::JOURNEY_CASES
        .iter()
        .any(|entry| entry.id == case)
}

#[cfg(test)]
pub(crate) use wire::{
    child_opened_wire_for_test, claude_sdk_wire_for_test, codex_user_input_wire_for_test,
    pi_journey_steps_for_test, replay_native, run_pi_step_for_test,
};

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<i32, FixtureError> {
    interpreter::execute(FixtureInvocation::parse(arguments)?)
}

#[cfg(test)]
mod tests;
