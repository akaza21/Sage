pub(crate) mod attached_context;
pub(crate) mod bindings;
#[cfg(feature = "acp-fixture")]
mod characterization_observation;
pub(crate) mod command_output;
pub mod contract;
pub(crate) mod declared_test;
pub(crate) mod file_diff;
#[cfg(any(test, feature = "acp-fixture"))]
pub mod fixture;
pub(crate) mod image_admit;
pub mod intent;
mod ipc;
mod lifecycle;
pub mod limits;
pub(crate) mod migration;
pub(crate) mod package_manifest;
pub(crate) mod page_admission;
pub(crate) mod process;
pub mod projection;
pub(crate) mod recovery;
pub(crate) mod redaction;
// The admitted runtime host constructs this snapshot; keep the independently
// reviewable packet warning-clean until that host integration lands.
#[allow(dead_code)]
pub(crate) mod resource_mapper;
pub(crate) mod runtime;
pub(crate) mod scheduler;
pub(crate) mod semantic_ingress;
pub(crate) mod service;
pub(crate) mod session_config;
pub(crate) mod store;
pub(crate) mod transport;

#[cfg(test)]
mod fixture_protocol;

#[cfg(test)]
mod tests {
    mod contract;
    mod limits;
}
