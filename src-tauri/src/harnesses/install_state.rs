use super::{HarnessCatalog, SIMULATOR_ID, SIMULATOR_IN_CATALOG, simulator_admission};
use crate::harness_discovery::known_harness_spec;
use sage_domain::harness::HarnessInstallState;

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

    /// `None` when the catalog does not list the harness.
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
            custom if crate::settings_store::custom_acp_number(custom).is_some() => {
                self.custom_acp_install_state(custom)
            }
            curated if known_harness_spec(curated).is_some() => Some(self.install_state(curated)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::fs;
    use std::sync::Arc;

    use super::super::test_support::by_id;
    use crate::{
        acp::bindings::{
            codex::{CodexBindingRegistration, CodexHostPresence},
            registry::BindingRegistry,
        },
        harnesses::{HarnessCatalog, SIMULATOR_ID},
    };
    use sage_domain::harness::{HarnessInstallState, HarnessUnavailableReason};

    #[test]
    fn the_typed_install_state_agrees_with_every_catalog_entry() {
        for catalog in [
            HarnessCatalog::default(),
            HarnessCatalog::default().settled_for_test(),
        ] {
            for simulator_in_catalog in [true, false] {
                let definitions = catalog.definitions_for(simulator_in_catalog);
                for definition in &definitions {
                    assert_eq!(
                        catalog
                            .install_state_for(&definition.id, simulator_in_catalog)
                            .map(HarnessInstallState::as_str),
                        Some(definition.install_state.as_str()),
                        "{}",
                        definition.id
                    );
                }
                assert_eq!(
                    catalog
                        .install_state_for(SIMULATOR_ID, simulator_in_catalog)
                        .is_some(),
                    definitions.iter().any(|entry| entry.id == SIMULATOR_ID)
                );
            }
            // Every harness with a state is listed, so neither side can grow alone.
            for id in crate::harness_discovery::KNOWN_HARNESS_SPECS
                .iter()
                .map(|spec| spec.id)
                .chain([SIMULATOR_ID])
            {
                for simulator_in_catalog in [true, false] {
                    assert_eq!(
                        catalog
                            .install_state_for(id, simulator_in_catalog)
                            .is_some(),
                        catalog
                            .definitions_for(simulator_in_catalog)
                            .iter()
                            .any(|entry| entry.id == id),
                        "{id}"
                    );
                }
            }
            assert_eq!(catalog.install_state_of("not-a-harness"), None);
        }
    }

    #[test]
    fn discovery_commits_claude_admission_with_the_catalog_snapshot() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        );

        assert_eq!(
            by_id(&catalog.definitions(), "claude-code").install_state,
            HarnessInstallState::Detecting.as_str()
        );

        catalog.apply_discovery(
            crate::harness_discovery::ClaudeLaunchDiscovery::AdapterAbsent,
            Vec::new(),
        );

        let claude = by_id(&catalog.definitions(), "claude-code");
        assert_eq!(
            claude.install_state,
            HarnessInstallState::NotInstalled.as_str()
        );
        assert_eq!(
            claude.unavailable_reason,
            Some(HarnessUnavailableReason::AdapterAbsent)
        );
    }

    #[cfg(windows)]
    fn admitted_opencode_catalog(root: &std::path::Path, version: &str) -> HarnessCatalog {
        let npm = root.join("npm");
        let shim = npm.join("opencode.cmd");
        let executable = npm
            .join("node_modules")
            .join("opencode-ai")
            .join("bin")
            .join("opencode.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&shim, format!("@echo off\r\necho {version}\r\n")).unwrap();
        fs::write(&executable, b"native-opencode").unwrap();
        fs::write(
            npm.join("node_modules")
                .join("opencode-ai")
                .join("package.json"),
            format!(
                "{{\"name\":\"opencode-ai\",\"version\":\"{version}\",\"bin\":{{\"opencode\":\"./bin/opencode.exe\"}}}}"
            ),
        )
        .unwrap();
        let registry = Arc::new(
            crate::acp::bindings::registry::BindingRegistry::new_with_opencode_for_test(
                CodexBindingRegistration::unavailable_for_test(),
                crate::acp::bindings::claude::ClaudeBindingRegistration::absent_for_test(),
                crate::acp::bindings::standard_acp::OpenCodeBindingRegistration::admitted_for_test(
                    &shim, version,
                ),
            ),
        );
        HarnessCatalog::with_registry(registry).settled_for_test()
    }

    #[cfg(windows)]
    #[test]
    fn admitted_opencode_publishes_a_drivable_characterised_binding() {
        let root =
            std::env::temp_dir().join(format!("sage-opencode-drivable-{}", uuid::Uuid::new_v4()));
        let catalog = admitted_opencode_catalog(&root, "1.18.21");
        let opencode = by_id(&catalog.definitions(), "opencode");
        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            opencode.install_state,
            HarnessInstallState::Available.as_str()
        );
        assert_eq!(opencode.binding_id.as_deref(), Some("opencode-acp"));
        assert_eq!(opencode.bound_version.as_deref(), Some("1.18.21"));
        assert_eq!(
            opencode.characterisation,
            Some(sage_domain::harness::HarnessCharacterisation::Characterised)
        );
        assert!(opencode.unavailable_reason.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn only_a_later_opencode_build_keeps_the_characterised_claims() {
        for (version, characterisation) in [
            (
                "1.18.20",
                sage_domain::harness::HarnessCharacterisation::Uncharacterised,
            ),
            (
                "1.19.0",
                sage_domain::harness::HarnessCharacterisation::NewerThanTested,
            ),
        ] {
            let root = std::env::temp_dir()
                .join(format!("sage-opencode-release-{}", uuid::Uuid::new_v4()));
            let catalog = admitted_opencode_catalog(&root, version);
            let opencode = by_id(&catalog.definitions(), "opencode");
            fs::remove_dir_all(root).unwrap();
            assert_eq!(
                opencode.install_state,
                HarnessInstallState::Available.as_str()
            );
            assert_eq!(opencode.binding_id.as_deref(), Some("opencode-acp"));
            assert_eq!(opencode.bound_version.as_deref(), Some(version));
            assert_eq!(opencode.characterisation, Some(characterisation));
            assert!(!opencode.account_model_discovery);
        }
    }

    #[test]
    fn catalog_reports_absent_codex_as_not_installed() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        )
        .settled_for_test();
        let codex = by_id(&catalog.definitions(), "codex");
        assert_eq!(codex.install_state, "notInstalled");
        assert!(catalog.models("codex").unwrap().is_empty());
    }

    #[test]
    fn an_unadmitted_binding_projects_a_redacted_reason_rather_than_silence() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Present,
        );
        let definitions = catalog.definitions();
        let codex = by_id(&definitions, "codex");
        assert!(codex.bound_version.is_none());
        assert!(codex.characterisation.is_none());
        assert_eq!(
            codex.unavailable_reason,
            Some(HarnessUnavailableReason::AdmissionIncomplete)
        );
        assert_eq!(
            by_id(&definitions, "claude-code").unavailable_reason,
            Some(sage_domain::harness::HarnessUnavailableReason::AdapterAbsent),
        );
        assert_eq!(
            by_id(&definitions, "opencode").unavailable_reason,
            Some(HarnessUnavailableReason::AdmissionIncomplete),
        );
        assert_eq!(
            by_id(&definitions, "pi").unavailable_reason,
            Some(HarnessUnavailableReason::AdmissionIncomplete),
        );
        for id in ["gemini-cli", "cursor-agent", "oh-my-pi"] {
            assert!(by_id(&definitions, id).unavailable_reason.is_none(), "{id}");
        }
    }

    #[test]
    fn a_bound_uncharacterised_build_is_projected_as_uncharacterised() {
        use crate::acp::bindings::claude::ClaudeBindingRegistration;
        use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};

        let root = std::env::temp_dir().join(format!(
            "sage-catalog-uncharacterised-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let adapter = root.join("adapter.js");
        let node = root.join("node.exe");
        std::fs::write(&adapter, b"adapter").unwrap();
        std::fs::write(&node, b"node").unwrap();
        let claude = ClaudeBindingRegistration::admitted_for_test(ClaudeLaunchDiscovery::Present(
            Box::new(DiscoveredClaudeLaunch {
                adapter_path: adapter.clone(),
                adapter_version: "0.75.0".into(),
                acp_sdk_version: "1.4.0".into(),
                node_engine: ">=22".into(),
                node_path: node.clone(),
                node_version: "22.18.0".into(),
                adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
                node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
            }),
        ));
        let catalog = HarnessCatalog::with_registry(Arc::new(BindingRegistry::new(
            CodexBindingRegistration::unavailable_for_test(),
            claude,
        )))
        .settled_for_test();
        let entry = by_id(&catalog.definitions(), "claude-code");
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(entry.install_state, "available");
        assert_eq!(
            entry.characterisation,
            Some(sage_domain::harness::HarnessCharacterisation::Uncharacterised)
        );
        assert_eq!(entry.bound_version.as_deref(), Some("0.75.0"));
        assert!(entry.unavailable_reason.is_none());
    }

    #[test]
    fn catalog_reports_present_unadmitted_codex_as_unavailable() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Present,
        )
        .settled_for_test();
        let codex = by_id(&catalog.definitions(), "codex");
        assert_eq!(codex.install_state, "unavailable");
        assert_eq!(codex.binding_id.as_deref(), Some("codex-app-server"));
        assert!(catalog.models("codex").unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn catalog_keeps_admitted_codex_available_without_host_presence() {
        let registration =
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            registration,
            CodexHostPresence::Absent,
        )
        .settled_for_test();
        let codex = by_id(&catalog.definitions(), "codex");
        assert_eq!(codex.install_state, "available");
        assert_eq!(codex.binding_id.as_deref(), Some("codex-app-server"));
        assert!(catalog.models("codex").unwrap().is_empty());
    }
}
