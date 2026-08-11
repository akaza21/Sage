//! Source-owned Codex App Server adapter packet.
//!
//! This module deliberately has no launcher. A validated candidate remains
//! unavailable until a separately admitted native artifact is supplied.

mod incoming;
mod managed;
mod manifest;
mod physical;
mod policy;
mod reproduction;
mod request;
mod runtime;
mod state;

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
pub(crate) use reproduction::{
    ReproductionError, bind_reproduced_package, parse_reproduction_requirements,
};
pub(crate) use runtime::CodexRuntime;
pub(crate) use state::CodexAdapter;

#[cfg(test)]
mod tests;
