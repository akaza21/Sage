//! Where each catalog harness stands: detecting, available, unavailable or
//! not installed. `definitions()` projects these states as strings; callers
//! that branch on a state use the typed value from `install_state_of`.

use super::{HarnessCatalog, SIMULATOR_ID, SIMULATOR_IN_CATALOG, simulator_admission};
use crate::{domain::HarnessInstallState, harness_discovery::known_harness_spec};

impl HarnessCatalog {
    pub(super) fn codex_install_state(&self) -> HarnessInstallState {
        if self.discovered.is_pending() {
            return HarnessInstallState::Detecting;
        }
        if self.registry.codex_admission().is_available() {
            HarnessInstallState::Available
        } else if self.discovered("codex").is_some() || self.observed_presence().is_present() {
            HarnessInstallState::Unavailable
        } else {
            HarnessInstallState::NotInstalled
        }
    }

    pub(super) fn install_state(&self, harness_id: &str) -> HarnessInstallState {
        if self.discovered.is_pending() {
            return HarnessInstallState::Detecting;
        }
        if self
            .registry
            .for_harness(harness_id)
            .is_some_and(|record| record.admission_available())
        {
            HarnessInstallState::Available
        } else if self.discovered(harness_id).is_some() {
            HarnessInstallState::Unavailable
        } else {
            HarnessInstallState::NotInstalled
        }
    }

    /// The install state of a harness in Sage's catalog, or `None` when the
    /// catalog does not list it. Agrees with `definitions()` entry for entry.
    pub(crate) fn install_state_of(&self, harness_id: &str) -> Option<HarnessInstallState> {
        self.install_state_for(harness_id, SIMULATOR_IN_CATALOG)
    }

    pub(super) fn install_state_for(
        &self,
        harness_id: &str,
        simulator_in_catalog: bool,
    ) -> Option<HarnessInstallState> {
        match harness_id {
            SIMULATOR_ID => simulator_admission(simulator_in_catalog),
            #[cfg(all(windows, feature = "acp-fixture"))]
            "sage-characterization" if self.characterization_available && simulator_in_catalog => {
                Some(HarnessInstallState::Available)
            }
            "codex" => Some(self.codex_install_state()),
            curated if known_harness_spec(curated).is_some() => Some(self.install_state(curated)),
            _ => None,
        }
    }
}
