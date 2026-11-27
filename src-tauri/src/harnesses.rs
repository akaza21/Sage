//! The harness catalog: the curated harness entries, their install state and
//! models, and the user's custom ACP agents. Each responsibility lives in a
//! child module; this root owns the catalog type and the simulator policy.

use std::sync::Arc;

mod builtin_commands;
mod catalog_definition;
mod catalog_lifecycle;
mod claude_auth;
mod curated_entries;
mod custom_acp;
mod draft_commands;
pub(crate) use draft_commands::record_draft_commands;
mod discovery_state;
mod install_state;
mod model_options;
mod persisted_binding;
mod published_releases;
#[cfg(test)]
mod test_support;
mod version_details;
use discovery_state::DiscoveryState;
use published_releases::PublishedReleases;

#[cfg(test)]
use crate::acp::bindings::codex::CodexHostPresence;
use crate::acp::bindings::registry::BindingRegistry;
use sage_domain::harness::HarnessInstallState;

pub const SIMULATOR_ID: &str = "sage-simulator";
pub const SIMULATOR_DEFAULT_MODEL_ID: &str = "sim-fast";
pub const SIMULATOR_DEEP_MODEL_ID: &str = "sim-deep";
pub const SIMULATOR_IN_CATALOG: bool = cfg!(debug_assertions);

fn simulator_admission(in_catalog: bool) -> Option<HarnessInstallState> {
    in_catalog.then_some(HarnessInstallState::Available)
}

#[derive(Debug, Clone)]
pub struct HarnessCatalog {
    registry: Arc<BindingRegistry>,
    discovered: DiscoveryState,
    published: PublishedReleases,
    rediscovery: Arc<std::sync::Mutex<()>>,
    #[cfg(test)]
    presence_override: Option<CodexHostPresence>,
    #[cfg(all(windows, feature = "acp-fixture"))]
    characterization_available: bool,
}

impl Default for HarnessCatalog {
    fn default() -> Self {
        Self::with_registry(Arc::new(BindingRegistry::default()))
    }
}
