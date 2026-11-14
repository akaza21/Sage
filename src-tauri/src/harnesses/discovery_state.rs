use std::sync::{Arc, RwLock};

use crate::{
    acp::bindings::{codex::admit_discovered_codex, registry::BindingRegistry},
    harness_discovery::{ClaudeLaunchDiscovery, InstalledHarness},
};

#[derive(Debug)]
pub(super) enum DiscoverySnapshot {
    Pending,
    Presence(Vec<InstalledHarness>),
    Discovered(Vec<InstalledHarness>),
}

#[derive(Debug, Clone)]
pub(super) struct DiscoveryState(Arc<RwLock<DiscoverySnapshot>>);

impl Default for DiscoveryState {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(DiscoverySnapshot::Pending)))
    }
}

impl DiscoveryState {
    /// A rediscovery keeps the last full result until it finishes.
    pub(super) fn apply_presence(&self, discovered: Vec<InstalledHarness>) {
        let mut snapshot = self.0.write().expect("harness discovery lock poisoned");
        if !matches!(*snapshot, DiscoverySnapshot::Discovered(_)) {
            *snapshot = DiscoverySnapshot::Presence(discovered);
        }
    }

    pub(super) fn apply(
        &self,
        registry: &BindingRegistry,
        claude: ClaudeLaunchDiscovery,
        discovered: Vec<InstalledHarness>,
    ) {
        let codex = discovered
            .iter()
            .find(|candidate| candidate.id == "codex")
            .cloned();
        if admit_discovered_codex(registry.codex_admission(), codex) {
            let _ = registry.codex_admission().refresh();
        }
        registry.claude_admission().apply_discovery(claude);
        registry.claude_sdk_admission().apply_discovery(
            discovered
                .iter()
                .find(|candidate| candidate.id == "claude-code")
                .and_then(|candidate| {
                    Some(InstalledHarness {
                        executable: super::claude_auth::native_claude_executable(
                            &candidate.executable,
                        )?,
                        ..candidate.clone()
                    })
                })
                .as_ref(),
        );
        registry.opencode_admission().apply_discovery(
            discovered
                .iter()
                .find(|candidate| candidate.id == "opencode"),
        );
        registry.pi_admission().apply_discovery(
            if discovered.iter().any(|candidate| candidate.id == "pi") {
                crate::harness_discovery::discover_pi_launch()
            } else {
                crate::harness_discovery::PiLaunchDiscovery::Absent
            },
        );
        *self.0.write().expect("harness discovery lock poisoned") =
            DiscoverySnapshot::Discovered(discovered);
    }

    #[cfg(test)]
    pub(super) fn replace(&self, discovered: Vec<InstalledHarness>) {
        *self.0.write().expect("harness discovery lock poisoned") =
            DiscoverySnapshot::Discovered(discovered);
    }

    pub(super) fn find(&self, harness_id: &str) -> Option<InstalledHarness> {
        match &*self.0.read().expect("harness discovery lock poisoned") {
            DiscoverySnapshot::Pending => None,
            DiscoverySnapshot::Presence(discovered) | DiscoverySnapshot::Discovered(discovered) => {
                discovered
                    .iter()
                    .find(|candidate| candidate.id == harness_id)
                    .cloned()
            }
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        matches!(
            *self.0.read().expect("harness discovery lock poisoned"),
            DiscoverySnapshot::Pending
        )
    }

    pub(super) fn is_version_checking(&self, harness_id: &str) -> bool {
        match &*self.0.read().expect("harness discovery lock poisoned") {
            DiscoverySnapshot::Presence(discovered) => discovered
                .iter()
                .any(|candidate| candidate.id == harness_id),
            DiscoverySnapshot::Pending | DiscoverySnapshot::Discovered(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_transitions_from_pending_to_discovered() {
        let state = DiscoveryState::default();
        assert!(state.is_pending());
        state.replace(Vec::new());
        assert!(!state.is_pending());
    }

    #[test]
    fn a_rediscovery_keeps_the_last_full_result_until_it_finishes() {
        let state = DiscoveryState::default();
        let codex = InstalledHarness {
            id: "codex",
            executable: std::path::PathBuf::from("codex"),
            version: Some("0.167.1".into()),
        };
        state.replace(vec![codex.clone()]);
        state.apply_presence(vec![InstalledHarness {
            version: None,
            ..codex
        }]);
        assert!(!state.is_version_checking("codex"));
        assert_eq!(
            state
                .find("codex")
                .and_then(|codex| codex.version)
                .as_deref(),
            Some("0.167.1")
        );
    }

    #[test]
    fn presence_marks_only_present_harnesses_as_version_checking() {
        let state = DiscoveryState::default();
        state.apply_presence(vec![InstalledHarness {
            id: "codex",
            executable: std::path::PathBuf::from("codex"),
            version: None,
        }]);

        assert!(!state.is_pending());
        assert!(state.is_version_checking("codex"));
        assert!(!state.is_version_checking("pi"));
        state.replace(Vec::new());
        assert!(!state.is_version_checking("codex"));
    }
}
