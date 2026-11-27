//! Construction of the catalog and the state it carries between commands:
//! discovery results, the rediscovery lock and the published-release check.

use std::sync::Arc;

use super::{DiscoveryState, HarnessCatalog, PublishedReleases};
use crate::{
    acp::bindings::{
        codex::{CodexHostPresence, observe_codex_host_presence},
        registry::BindingRegistry,
    },
    harness_discovery::InstalledHarness,
};

#[cfg(test)]
use crate::acp::bindings::codex::CodexBindingRegistration;
#[cfg(all(test, windows, feature = "acp-fixture"))]
use crate::acp::bindings::codex::startup_codex_binding_registration;

impl HarnessCatalog {
    pub(crate) fn with_registry(registry: Arc<BindingRegistry>) -> Self {
        Self {
            registry,
            discovered: DiscoveryState::default(),
            published: PublishedReleases::default(),
            rediscovery: Arc::default(),
            #[cfg(test)]
            presence_override: None,
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization_available: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_codex_registration(codex: CodexBindingRegistration) -> Self {
        Self::with_registry(Arc::new(BindingRegistry::with_codex(codex)))
    }

    #[cfg(test)]
    pub(crate) fn with_codex_registration_and_presence(
        codex: CodexBindingRegistration,
        presence: CodexHostPresence,
    ) -> Self {
        Self {
            registry: Arc::new(BindingRegistry::with_codex(codex)),
            discovered: DiscoveryState::default(),
            published: PublishedReleases::default(),
            rediscovery: Arc::default(),
            presence_override: Some(presence),
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization_available: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn settled_for_test(self) -> Self {
        self.discovered.replace(Vec::new());
        self
    }

    pub(crate) fn refresh_codex_registration(
        &self,
    ) -> Result<(), crate::acp::bindings::codex::CodexAdmissionUnavailable> {
        self.registry.codex_admission().refresh().map(drop)
    }

    pub(crate) fn apply_discovery(
        &self,
        claude: crate::harness_discovery::ClaudeLaunchDiscovery,
        discovered: Vec<InstalledHarness>,
    ) {
        self.discovered.apply(&self.registry, claude, discovered);
    }

    /// Admits one harness whose probe answered while the others still run.
    pub(crate) fn apply_probed(&self, probed: &InstalledHarness) {
        self.discovered.apply_probed(&self.registry, probed);
    }

    pub(crate) fn apply_presence(&self, discovered: Vec<InstalledHarness>) {
        self.discovered.apply_presence(discovered);
    }

    /// Held while one discovery runs, so a refresh and the startup discovery
    /// never interleave and an earlier result never overwrites a later one.
    pub(crate) fn lock_rediscovery(&self) -> std::sync::MutexGuard<'_, ()> {
        self.rediscovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `None` while a recent check covers them; otherwise the check's epoch.
    pub(crate) fn begin_update_check(&self, harness_ids: &[&'static str]) -> Option<u64> {
        self.published.begin_check(harness_ids)
    }

    pub(crate) fn finish_update_check(
        &self,
        epoch: u64,
        found: Option<Vec<crate::harness_discovery::PublishedRelease>>,
    ) {
        self.published.finish_check(epoch, found);
    }

    pub(crate) fn forget_published_releases(&self) {
        self.published.forget();
    }

    #[cfg(test)]
    pub(crate) fn codex_registration(&self) -> CodexBindingRegistration {
        self.registry.codex_admission().clone()
    }

    #[cfg(test)]
    pub(crate) fn binding_registry(&self) -> &Arc<BindingRegistry> {
        &self.registry
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn with_characterization() -> Self {
        Self::with_registry_and_characterization(Arc::new(BindingRegistry::with_codex(
            startup_codex_binding_registration(),
        )))
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn with_registry_and_characterization(registry: Arc<BindingRegistry>) -> Self {
        Self {
            registry,
            discovered: DiscoveryState::default(),
            published: PublishedReleases::default(),
            rediscovery: Arc::default(),
            #[cfg(test)]
            presence_override: None,
            characterization_available: true,
        }
    }

    pub(super) fn observed_presence(&self) -> CodexHostPresence {
        #[cfg(test)]
        if let Some(presence) = self.presence_override {
            return presence;
        }
        observe_codex_host_presence()
    }

    pub(super) fn discovered(&self, harness_id: &str) -> Option<InstalledHarness> {
        self.discovered.find(harness_id)
    }
}
