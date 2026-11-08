//! Conservative H3 translation for the pinned public Claude ACP adapter.
//!
//! This module does not launch Claude and does not treat MCP, nested transcripts,
//! or adapter metadata as session-transport authority. Runtime admission remains
//! blocked until the complete immutable artifact graph is independently reviewed.

mod admission;
mod auth;
mod capabilities;
mod manifest;
mod runtime;

pub(crate) use crate::acp::bindings::standard_acp::{sanitize, translator, wire};

use crate::acp::bindings::standard_acp::{StandardAcpProfile, profile_for};

pub(crate) const CLAUDE_PROFILE_ID: &str = "claude-code";

pub(crate) fn claude_profile() -> &'static StandardAcpProfile {
    profile_for(CLAUDE_PROFILE_ID).expect("claude profile is a compiled-in row")
}

pub(crate) use admission::{
    AdmittedClaudeBinding, ClaudeAdmissionUnavailable, ClaudeBindingRegistration,
};
pub(crate) use auth::{ClaudeAuthState, decode_auth_required_error, decode_auth_status_update};
pub(crate) use capabilities::{ClaudeCapabilityOffer, negotiate_capabilities};
pub(crate) use manifest::{
    AdmittedCandidate, CLAUDE_BINDING_ID, CandidateManifest, ENTRYPOINT, LAUNCH_ENVIRONMENT_KEYS,
    ManifestError, validate_research_candidate,
};
pub(crate) use runtime::ClaudeRuntimeFactory;
#[cfg(test)]
pub(crate) use runtime::InMemoryClaude;
#[cfg(test)]
pub(crate) use translator::TranslationError;
pub(crate) use translator::{
    AdapterEvent, PromptTerminal, ResourceMapper, StandardAcpTranslator as ClaudeTranslator,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod snapshot_mapping_tests;
