//! Pi JSONL/RPC binding.
//!
//! Pi does not expose first-party ACP. This module keeps Pi wire types
//! adapter-local and translates only the behavior proven by the curated RPC
//! contract (pinned at v0.83.0, admitted from v0.74.0). The product admits it
//! read-only through `native_admission`: Pi launches with only its `read` tool
//! and the translator still rejects every other tool.

mod decode;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) mod fixture_admission;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) mod fixture_runtime;
#[cfg(all(test, windows, feature = "acp-fixture", debug_assertions))]
mod fixture_tests;
mod manifest;
pub(crate) mod native_admission;
pub(crate) mod native_runtime;
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
