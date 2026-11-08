mod admission;
mod custom;
pub(crate) mod custom_executable;
pub(crate) mod elicitation;
mod environment;
mod handshake;
mod launch;
mod opencode;
mod opencode_policy;
mod profile;
mod runtime;
pub(crate) mod sanitize;
pub(crate) mod surface;
pub(crate) mod translator;
pub(crate) mod wire;

#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod diff_drain_tests;
#[cfg(test)]
mod manifest_launch_tests;
#[cfg(test)]
mod open_items_tests;
#[cfg(test)]
mod permission_summary_tests;
#[cfg(all(test, windows))]
mod real_binary_tests;
#[cfg(test)]
mod render_contract_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tool_input_tests;
#[cfg(test)]
mod web_search_tests;

pub(crate) use admission::{
    AdmittedStandardAcpBinding, StandardAcpAdmissionError, StandardAcpArtifact,
    StandardAcpArtifactProvenance, StandardAcpCandidate, admit,
};
pub(crate) use custom::{CUSTOM_ACP_POLICY_ID, CustomAcpRegistration, CustomAcpUnavailable};
pub(crate) use handshake::{StandardAcpHandshake, StandardAcpHandshakeError};
pub(crate) use launch::{AcpLaunchAuthority, ClaudeLaunch};
#[cfg(test)]
pub(crate) use opencode::candidate_from_global_npm_shim;
pub(crate) use opencode::{
    OPENCODE_BINDING_ID, OpenCodeAdmissionUnavailable, OpenCodeBindingRegistration,
};
#[cfg(test)]
pub(crate) use profile::PROFILES;
pub(crate) use profile::{
    NpmPackageManifestRequirement, StandardAcpLaunch, StandardAcpProfile, profile_for,
};
pub(crate) use runtime::{StandardAcpBindingSource, StandardAcpRuntimeFactory};
