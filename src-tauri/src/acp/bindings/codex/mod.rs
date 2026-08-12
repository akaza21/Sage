//! Source-owned Codex App Server adapter packet.
//!
//! This module deliberately has no launcher. A validated candidate remains
//! unavailable until a separately admitted native artifact is supplied.

mod admission;
mod authority;
mod discovery;
mod incoming;
mod managed;
mod manifest;
mod physical;
mod policy;
mod provisioning;
mod reproduction;
mod request;
mod runtime;
mod state;

pub(crate) use admission::{
    AdmittedCodexBinding, CodexAdmissionUnavailable, CodexBindingRegistration,
    startup_codex_binding_registration,
};
pub(crate) use authority::{
    ConfigAuthorityError, EnvironmentAuthorityError, ResidueMarker, RetentionDecision,
    StateAuthorityError, build_codex_launch_environment, classify_codex_state_path,
    inspect_codex_authority, verify_codex_config,
};
pub(crate) use discovery::{
    AdmittedDiscovery, CodexAccountSnapshot, CodexDiscoveryContent, CodexDiscoveryDeadline,
    CodexDiscoveryRunError, CodexModelSnapshot, discover_admitted,
};
#[cfg(test)]
pub(crate) use discovery::{
    CodexDiscoveryProtocol, DiscoveryTransport, DiscoveryZeroProcess, RevalidationCheckpoint,
    run_discovery_protocol,
};
pub(crate) use incoming::{IncomingEvent, IngressDisposition, ResourceMapper};
pub(crate) use managed::{
    ManagedStagingError, ManagedStagingPrerequisite, inspect_managed_staging,
};
pub(crate) use manifest::{PhysicalAdmissionUnavailable, Platform, parse_packet_metadata};
pub(crate) use physical::{
    ConfiguredManagedPackageRoot, PhysicalInspectionError, PhysicallyInspectedPackage,
    inspect_managed_package,
};
pub(crate) use policy::{SecurityPolicyError, parse_security_policy};
pub(crate) use provisioning::{
    ExplicitStagingAuthorization, PreparedCodexStaging, ProvisioningError, VerifiedCodexSource,
    authorize_offline_staging_from_environment, prepare_codex_staging, verify_codex_source,
};
pub(crate) use reproduction::{
    ReproductionError, bind_reproduced_package, parse_reproduction_requirements,
};
pub(crate) use runtime::{CodexRuntime, authority::CodexProjectRootAuthority};
pub(crate) use state::CodexAdapter;

#[cfg(test)]
mod tests;
