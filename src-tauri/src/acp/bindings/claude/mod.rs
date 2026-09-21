//! Conservative H3 translation for the pinned public Claude ACP adapter.
//!
//! This module does not launch Claude and does not treat MCP, nested transcripts,
//! or adapter metadata as session-transport authority. Runtime admission remains
//! blocked until the complete immutable artifact graph is independently reviewed.

mod admission;
mod capabilities;
mod manifest;
mod runtime;
mod sanitize;
mod translator;
mod wire;

pub(crate) use admission::{
    AdmittedClaudeBinding, ClaudeAdmissionUnavailable, ClaudeBindingRegistration,
};
pub(crate) use capabilities::{ClaudeCapabilityOffer, negotiate_capabilities};
pub(crate) use manifest::{
    ACP_SDK_VERSION, AdmittedCandidate, CLAUDE_BINDING_ID, CandidateManifest, ENTRYPOINT,
    ManifestError, validate_research_candidate,
};
pub(crate) use runtime::{ClaudeRuntime, ClaudeRuntimeFactory};
pub(crate) use translator::{
    AdapterEvent, ClaudeTranslator, PromptTerminal, ResourceMapper, TranslationError,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod snapshot_mapping_tests;
