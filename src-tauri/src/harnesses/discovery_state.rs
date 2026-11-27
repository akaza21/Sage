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

#[derive(Debug)]
pub(super) struct DiscoveryProgress {
    snapshot: DiscoverySnapshot,
    /// The present harnesses this discovery has already admitted; each is
    /// admitted once, as soon as its own probe answers.
    admitted: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub(super) struct DiscoveryState(Arc<RwLock<DiscoveryProgress>>);

impl Default for DiscoveryState {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(DiscoveryProgress {
            snapshot: DiscoverySnapshot::Pending,
            admitted: Vec::new(),
        })))
    }
}

/// The curated harnesses discovery admits; one absent from a discovery is
/// admitted as absent when it settles.
const DISCOVERED_HARNESSES: [&str; 4] = ["codex", "claude-code", "opencode", "pi"];

fn admit(registry: &BindingRegistry, id: &str, discovered: Option<&InstalledHarness>) {
    match id {
        "codex" => {
            if admit_discovered_codex(registry.codex_admission(), discovered.cloned()) {
                let _ = registry.codex_admission().refresh();
            }
        }
        "claude-code" => registry.claude_sdk_admission().apply_discovery(
            discovered
                .and_then(|candidate| {
                    Some(InstalledHarness {
                        executable: super::claude_auth::native_claude_executable(
                            &candidate.executable,
                        )?,
                        ..candidate.clone()
                    })
                })
                .as_ref(),
        ),
        "opencode" => registry.opencode_admission().apply_discovery(discovered),
        "pi" => registry
            .pi_admission()
            .apply_discovery(if discovered.is_some() {
                crate::harness_discovery::discover_pi_launch()
            } else {
                crate::harness_discovery::PiLaunchDiscovery::Absent
            }),
        _ => {}
    }
}

impl DiscoveryState {
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, DiscoveryProgress> {
        self.0.write().expect("harness discovery lock poisoned")
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, DiscoveryProgress> {
        self.0.read().expect("harness discovery lock poisoned")
    }

    /// A rediscovery keeps the last full result until it finishes.
    pub(super) fn apply_presence(&self, discovered: Vec<InstalledHarness>) {
        let mut progress = self.write();
        progress.admitted.clear();
        if !matches!(progress.snapshot, DiscoverySnapshot::Discovered(_)) {
            progress.snapshot = DiscoverySnapshot::Presence(discovered);
        }
    }

    /// Admits one present harness as soon as its probe answered, while the
    /// others are still being probed.
    pub(super) fn apply_probed(&self, registry: &BindingRegistry, probed: &InstalledHarness) {
        admit(registry, probed.id, Some(probed));
        let mut progress = self.write();
        progress.admitted.push(probed.id);
        if let DiscoverySnapshot::Presence(harnesses) | DiscoverySnapshot::Discovered(harnesses) =
            &mut progress.snapshot
        {
            match harnesses.iter_mut().find(|known| known.id == probed.id) {
                Some(known) => *known = probed.clone(),
                None => harnesses.push(probed.clone()),
            }
        }
    }

    pub(super) fn apply(
        &self,
        registry: &BindingRegistry,
        claude: ClaudeLaunchDiscovery,
        discovered: Vec<InstalledHarness>,
    ) {
        registry.claude_admission().apply_discovery(claude);
        let admitted = std::mem::take(&mut self.write().admitted);
        for id in DISCOVERED_HARNESSES {
            if !admitted.contains(&id) {
                admit(
                    registry,
                    id,
                    discovered.iter().find(|candidate| candidate.id == id),
                );
            }
        }
        self.write().snapshot = DiscoverySnapshot::Discovered(discovered);
    }

    #[cfg(test)]
    pub(super) fn replace(&self, discovered: Vec<InstalledHarness>) {
        self.write().snapshot = DiscoverySnapshot::Discovered(discovered);
    }

    pub(super) fn find(&self, harness_id: &str) -> Option<InstalledHarness> {
        match &self.read().snapshot {
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
        matches!(self.read().snapshot, DiscoverySnapshot::Pending)
    }

    pub(super) fn is_version_checking(&self, harness_id: &str) -> bool {
        let progress = self.read();
        match &progress.snapshot {
            DiscoverySnapshot::Presence(discovered) => {
                !progress.admitted.iter().any(|id| *id == harness_id)
                    && discovered
                        .iter()
                        .any(|candidate| candidate.id == harness_id)
            }
            DiscoverySnapshot::Pending | DiscoverySnapshot::Discovered(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn present(id: &'static str) -> InstalledHarness {
        InstalledHarness {
            id,
            executable: std::path::PathBuf::from(id),
            version: None,
        }
    }

    #[test]
    fn a_probed_harness_stops_checking_while_the_others_are_still_probed() {
        let registry = BindingRegistry::default();
        let state = DiscoveryState::default();
        state.apply_presence(vec![present("gemini"), present("cursor")]);
        state.apply_probed(
            &registry,
            &InstalledHarness {
                version: Some("1.0.0".into()),
                ..present("gemini")
            },
        );
        assert!(!state.is_version_checking("gemini"));
        assert!(state.is_version_checking("cursor"));
        assert_eq!(
            state.find("gemini").and_then(|h| h.version).as_deref(),
            Some("1.0.0")
        );
        state.apply(
            &registry,
            ClaudeLaunchDiscovery::AdapterAbsent,
            vec![present("gemini"), present("cursor")],
        );
        assert!(!state.is_version_checking("cursor"));
    }

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
