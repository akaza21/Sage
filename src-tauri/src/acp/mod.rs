pub(crate) mod attached_context;
pub(crate) mod bindings;
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
)]
pub mod browser;
#[cfg(feature = "acp-fixture")]
mod characterization_observation;
pub(crate) mod command_output;
pub mod contract;
pub(crate) mod declared_test;
pub(crate) mod file_diff;
#[cfg(any(test, feature = "acp-fixture"))]
pub mod fixture;
pub(crate) mod goal;
pub(crate) mod image_admit;
pub mod intent;
mod ipc;
mod lifecycle;
pub mod limits;
pub(crate) mod mcp;
pub(crate) mod migration;
pub(crate) mod package_manifest;
pub(crate) mod page_admission;
pub(crate) mod process;
pub mod projection;
pub(crate) mod prompt_mentions;
pub(crate) mod recovery;
pub(crate) mod redaction;
pub(crate) mod resource_mapper;
pub(crate) mod runtime;
pub(crate) mod scheduler;
pub(crate) mod semantic_ingress;
pub(crate) mod service;
pub(crate) mod session_action;
pub(crate) mod session_config;
pub(crate) mod session_control;
pub(crate) use bindings::adapter::slash_commands;
pub(crate) mod store;
pub(crate) mod transcript_quote;
pub(crate) mod transport;
pub(crate) mod usage;

#[cfg(all(test, feature = "acp-fixture"))]
mod fixture_protocol;

#[cfg(test)]
mod tests {
    mod contract;
    mod limits;
    mod render_parity;
}
