//! Pi v0.83.0 JSONL/RPC binding.
//!
//! Pi does not expose first-party ACP at the pinned version. This module keeps
//! Pi wire types adapter-local and translates only the behavior proven by the
//! curated RPC contract. It is deliberately not wired into the product until
//! ADE-0006C's artifact and read-only containment gates are accepted.

mod decode;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) mod fixture_admission;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) mod fixture_runtime;
#[cfg(all(test, windows, feature = "acp-fixture", debug_assertions))]
mod fixture_tests;
mod manifest;
mod protocol;
mod runtime;
mod translator;

#[allow(unused_imports)]
pub use manifest::{
    ArtifactCandidate, ArtifactIdentity, CuratedPiBinding, PI_LICENSE, PI_NODE_MINIMUM, PI_PACKAGE,
    PI_RPC_ARGS, PI_SOURCE_COMMIT, PI_VERSION, PiBindingCapabilities, PiBindingError, PiPlatform,
    ResearchPiBinding,
};
#[allow(unused_imports)]
pub use protocol::{
    AdapterError, AdapterOutput, EntrySummary, HistoryMessage, HistoryRole, ModelSummary,
    Operation, Outbound, StateSnapshot, TerminalOutcome, ToolOutcome,
};
pub(crate) use runtime::PiRuntime;
pub use translator::PiTranslator;

#[cfg(test)]
mod tests;
