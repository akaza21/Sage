//! Source-owned Codex App Server adapter packet.
//!
//! This module deliberately has no launcher. A validated candidate remains
//! unavailable until a separately admitted native artifact is supplied.

mod admission;
mod auth;
mod authority;
mod discovery;
mod host_presence;
mod incoming;
mod installed_role;
mod managed;
mod manifest;
mod physical;
mod policy;
mod provisioning;
mod reproduction;
mod request;
mod runtime;
mod state;
mod trust_root;

// Codex Responses dynamic function names cannot contain dots. This alias is
// confined to the Codex adapter; shared consumers use the ACP extension method
// `_sage/browse`.
const CODEX_BROWSE_TOOL_NAME: &str = "sage_browse";

pub(crate) use admission::BINDING_ID as CODEX_BINDING_ID;
pub(crate) use admission::{
    AdmittedCodexBinding, CodexAdmissionUnavailable, CodexBindingRegistration,
    startup_codex_binding_registration,
};
pub(crate) use auth::{
    AuthBrowser, AuthTransport, CodexAuthDeadline, CodexAuthError, CodexAuthNotification,
    CodexAuthOutcome, CodexAuthProtocol, CodexAuthRequest, NativeAuthUrl, NativeBrowser,
    NativeLoginId, ProductionAuthTransport, run_account_protocol, run_login_protocol,
    run_logout_protocol,
};
pub(crate) use authority::{
    ConfigAuthorityError, EnvironmentAuthorityError, ResidueMarker, RetentionDecision,
    StateAuthorityError, build_codex_launch_environment, classify_codex_state_path,
    inspect_codex_authority, verify_codex_config,
};
pub(crate) use discovery::{
    AdmittedDiscovery, CodexAccountSnapshot, CodexDiscoveryContent, CodexDiscoveryDeadline,
    CodexDiscoveryRunError, CodexModelSnapshot, CodexProcessTransport, DiscoveryZeroProcess,
    ProcessLaunchPolicy, discover_admitted,
};
#[cfg(test)]
pub(crate) use discovery::{
    CodexDiscoveryProtocol, DiscoveryTransport, RevalidationCheckpoint, run_discovery_protocol,
};
pub(crate) use host_presence::{CodexHostPresence, observe_codex_host_presence};
pub(crate) use incoming::{IncomingEvent, IngressDisposition, ResourceMapper};
pub(crate) use managed::{
    ManagedStagingError, ManagedStagingPrerequisite, inspect_managed_staging,
};
pub(crate) use manifest::{
    PINNED_PROTOCOL_IDENTITY as CODEX_PROTOCOL_IDENTITY,
    PINNED_SCHEMA_SHA256 as CODEX_SCHEMA_SHA256,
};
pub(crate) use manifest::{PhysicalAdmissionUnavailable, Platform, parse_packet_metadata};
pub(crate) use physical::{
    ConfiguredManagedPackageRoot, PhysicalInspectionError, PhysicallyInspectedPackage,
    inspect_managed_package,
};
pub(crate) use policy::{SecurityPolicyError, parse_security_policy};
pub(crate) use provisioning::{
    ExplicitStagingAuthorization, PreparedCodexStaging, ProvisioningError, StagingOperation,
    VerifiedCodexSource, authorize_offline_staging_from_environment, prepare_codex_staging,
    prepare_codex_staging_after_signed_role, verify_codex_source,
};
#[cfg(all(windows, feature = "codex-provisioning-helper"))]
pub(crate) use provisioning::{
    HelperOperation, execute_boundary_publication, execute_boundary_recover,
    execute_boundary_remove, execute_boundary_update, helper_operation,
};
pub(crate) use reproduction::{
    ReproductionError, bind_reproduced_package, parse_reproduction_requirements,
};
pub(crate) use runtime::{CodexRuntime, CodexRuntimeFactory, authority::CodexProjectRootAuthority};
pub(crate) use state::CodexAdapter;

#[cfg(test)]
mod tests;
