#![cfg(feature = "acp-fixture")]

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

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<i32, FixtureError> {
    interpreter::execute(FixtureInvocation::parse(arguments)?)
}

#[cfg(test)]
mod tests;
