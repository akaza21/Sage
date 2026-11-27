//! The models a listed harness offers, the default harness choice, and the
//! harness a persisted binding identity belongs to.

use super::{
    HarnessCatalog, SIMULATOR_DEEP_MODEL_ID, SIMULATOR_DEFAULT_MODEL_ID, SIMULATOR_ID,
    SIMULATOR_IN_CATALOG,
};
use sage_core::error::{AppError, AppResult};
use sage_domain::harness::{HarnessDefinition, HarnessTransport, ModelOption};

impl HarnessCatalog {
    /// The models a harness offers before a session, as its binding lists
    /// them; this may start a bounded probe of the installed build, so call
    /// it off the main thread.
    pub fn models(&self, harness_id: &str) -> AppResult<Vec<ModelOption>> {
        self.models_for(harness_id, SIMULATOR_IN_CATALOG, true)
    }

    pub fn known_models(&self, harness_id: &str) -> AppResult<Vec<ModelOption>> {
        self.models_for(harness_id, SIMULATOR_IN_CATALOG, false)
    }

    /// The harness's name when its build should list models and its latest
    /// probe listed none (it may still have been starting).
    pub fn models_unlisted(&self, harness_id: &str) -> Option<String> {
        self.registry
            .for_harness(harness_id)
            .filter(|record| record.launch_models_unlisted())?;
        self.definitions()
            .into_iter()
            .find(|definition| {
                definition.id == harness_id && definition.install_state == "available"
            })
            .map(|definition| definition.name)
    }

    fn models_for(
        &self,
        harness_id: &str,
        simulator_in_catalog: bool,
        probe: bool,
    ) -> AppResult<Vec<ModelOption>> {
        if harness_id == SIMULATOR_ID && simulator_in_catalog {
            return Ok(vec![
                ModelOption {
                    id: SIMULATOR_DEFAULT_MODEL_ID.into(),
                    name: "Simulator Fast".into(),
                    description: Some(
                        "Short deterministic responses for the walking skeleton.".into(),
                    ),
                    is_default: Some(true),
                    context_window: None,
                    effort_levels: None,
                },
                ModelOption {
                    id: SIMULATOR_DEEP_MODEL_ID.into(),
                    name: "Simulator Deep".into(),
                    description: Some("A second option for testing model changes.".into()),
                    is_default: Some(false),
                    context_window: None,
                    effort_levels: None,
                },
            ]);
        }

        let definition = self
            .definitions_for(simulator_in_catalog)
            .into_iter()
            .find(|definition| definition.id == harness_id)
            .ok_or_else(|| AppError::UnknownHarness(harness_id.into()))?;

        if definition.install_state == "available"
            && let Some(models) = self
                .registry
                .for_harness(harness_id)
                .and_then(|record| record.launch_models(probe))
        {
            return Ok(models);
        }
        if definition.install_state != "available" || definition.transport == HarnessTransport::Acp
        {
            return Ok(Vec::new());
        }

        Err(AppError::HarnessUnavailable(harness_id.into()))
    }

    pub(crate) fn preferred_available_harness_id(
        definitions: &[HarnessDefinition],
    ) -> Option<String> {
        definitions
            .iter()
            .find(|harness| harness.install_state == "available")
            .map(|harness| harness.id.clone())
    }

    pub fn harness_id_for_binding(&self, binding_id: &str) -> Option<String> {
        #[cfg(all(windows, feature = "acp-fixture"))]
        if self.characterization_available
            && binding_id == crate::acp::service::CHARACTERIZATION_BINDING_ID
        {
            return Some("sage-characterization".into());
        }
        if let Some(id) = self.registry.pending_claude_harness(binding_id) {
            return Some(id.into());
        }
        self.definitions()
            .into_iter()
            .find(|definition| {
                definition.binding_id.as_deref() == Some(binding_id)
                    || (definition.transport == HarnessTransport::LegacySimulator
                        && definition.id == binding_id)
            })
            .map(|definition| definition.id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::test_support::by_id;
    use crate::{
        acp::bindings::{
            codex::{CodexBindingRegistration, CodexHostPresence},
            registry::BindingRegistry,
        },
        harnesses::{HarnessCatalog, SIMULATOR_ID, SIMULATOR_IN_CATALOG},
    };
    use sage_core::error::AppError;
    use sage_domain::harness::{HarnessInstallState, ModelOption};

    #[test]
    fn model_options_are_scoped_to_the_selected_harness() {
        let catalog = HarnessCatalog::default();
        let models = catalog.models(SIMULATOR_ID).unwrap();
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|model| model.context_window.is_none()));
        let serialized = serde_json::to_value(&models).unwrap();
        assert!(
            serialized
                .as_array()
                .unwrap()
                .iter()
                .all(|model| model.get("contextWindow").is_none())
        );
        let known = ModelOption {
            id: "model-known".into(),
            name: "Known".into(),
            description: None,
            is_default: None,
            context_window: Some(272_000),
            effort_levels: None,
        };
        assert_eq!(
            serde_json::to_value(known).unwrap()["contextWindow"],
            272_000
        );
        assert!(catalog.models("codex").unwrap().is_empty());
        assert!(catalog.models("oh-my-pi").unwrap().is_empty());
    }

    #[test]
    fn unknown_harness_is_rejected() {
        let catalog = HarnessCatalog::default();
        assert!(matches!(
            catalog.models("unknown"),
            Err(AppError::UnknownHarness(_))
        ));
    }

    #[test]
    fn curated_binding_identity_resolves_to_its_harness_identity() {
        let catalog = HarnessCatalog::default();
        assert_eq!(
            catalog
                .harness_id_for_binding("codex-app-server")
                .as_deref(),
            Some("codex")
        );
        assert_eq!(
            catalog.harness_id_for_binding(SIMULATOR_ID).as_deref(),
            Some(SIMULATOR_ID)
        );
        assert_eq!(catalog.harness_id_for_binding("unknown"), None);
    }

    #[test]
    fn a_claude_chat_restored_before_discovery_keeps_its_harness() {
        let registry = Arc::new(BindingRegistry::default());
        let catalog = HarnessCatalog::with_registry(Arc::clone(&registry));
        let sdk = crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID;
        let acp = crate::acp::bindings::claude::CLAUDE_BINDING_ID;
        let mut definitions = catalog.definitions();
        for binding in [sdk, acp] {
            assert_eq!(
                catalog.restore_binding(binding, &mut definitions),
                "claude-code"
            );
        }
        assert!(
            definitions
                .iter()
                .all(|entry| !entry.id.starts_with("unavailable-binding:"))
        );
        registry.claude_sdk_admission().apply_discovery(None);
        assert_eq!(
            catalog.harness_id_for_binding(sdk).as_deref(),
            Some("claude-code")
        );
        registry.claude_admission().apply_discovery(
            crate::harness_discovery::ClaudeLaunchDiscovery::AdapterVersionUnreadable,
        );
        assert_eq!(catalog.harness_id_for_binding(sdk), None);
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    #[test]
    fn characterization_catalog_exists_only_for_an_admitted_launch_binding() {
        let default = HarnessCatalog::default();
        assert!(
            default
                .definitions()
                .iter()
                .all(|definition| definition.id != "sage-characterization")
        );
        assert_eq!(
            default.harness_id_for_binding(crate::acp::service::CHARACTERIZATION_BINDING_ID),
            None
        );

        let admitted = HarnessCatalog::with_characterization();
        assert_eq!(
            admitted.harness_id_for_binding(crate::acp::service::CHARACTERIZATION_BINDING_ID),
            Some("sage-characterization".into())
        );
        assert!(admitted.models("sage-characterization").unwrap().is_empty());
    }

    #[test]
    fn release_policy_omits_and_refuses_the_simulator() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        )
        .settled_for_test();
        assert!(
            catalog
                .definitions_for(false)
                .iter()
                .all(|definition| definition.id != SIMULATOR_ID)
        );
        assert!(matches!(
            catalog.models_for(SIMULATOR_ID, false, true),
            Err(AppError::UnknownHarness(_))
        ));
        assert_ne!(
            HarnessCatalog::preferred_available_harness_id(&catalog.definitions_for(false))
                .as_deref(),
            Some(SIMULATOR_ID)
        );
    }

    #[cfg(windows)]
    #[test]
    fn development_default_prefers_an_available_curated_harness() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::admitted_for_test(),
            CodexHostPresence::Present,
        )
        .settled_for_test();

        assert_eq!(
            HarnessCatalog::preferred_available_harness_id(&catalog.definitions_for(true))
                .as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn configured_catalog_matches_the_current_build_policy() {
        let definitions = HarnessCatalog::default().definitions();
        let default_id = HarnessCatalog::preferred_available_harness_id(&definitions);
        if SIMULATOR_IN_CATALOG {
            let simulator = by_id(&definitions, SIMULATOR_ID);
            assert_eq!(
                simulator.install_state,
                HarnessInstallState::Available.as_str()
            );
            let curated = definitions.iter().find(|definition| {
                definition.install_state == "available" && definition.source != "development"
            });
            assert_eq!(
                default_id.as_deref(),
                curated
                    .map(|definition| definition.id.as_str())
                    .or(Some(SIMULATOR_ID))
            );
        } else {
            assert!(
                definitions
                    .iter()
                    .all(|definition| definition.id != SIMULATOR_ID)
            );
            assert_ne!(default_id.as_deref(), Some(SIMULATOR_ID));
            if let Some(id) = default_id {
                assert_eq!(by_id(&definitions, &id).install_state, "available");
            }
        }
    }

    #[test]
    fn catalog_lists_the_models_the_admitted_codex_build_listed() {
        let executable = std::env::temp_dir().join(format!(
            "sage-codex-catalog-models-{}.bin",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&executable, b"observed-codex").unwrap();
        let registration = CodexBindingRegistration::unavailable_for_test();
        let binding = registration
            .authenticate_observed_for_test(executable.clone(), "codex-cli 0.167.1".into())
            .unwrap();
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            registration.clone(),
            CodexHostPresence::Present,
        )
        .settled_for_test();
        let listed = registration.model_catalog();
        let model = || {
            vec![ModelOption {
                id: "gpt-6.1-sol".into(),
                name: "GPT-6.1-Sol".into(),
                description: None,
                is_default: Some(true),
                context_window: None,
                effort_levels: Some(vec!["low".into(), "max".into(), "ultra".into()]),
            }]
        };
        assert!(catalog.known_models("codex").unwrap().is_empty());

        listed.refresh(listed.generation(), [0; 32], model());
        assert!(catalog.known_models("codex").unwrap().is_empty());

        listed.refresh(listed.generation(), binding.binding_digest(), model());
        let expected = serde_json::json!([{
            "id": "gpt-6.1-sol",
            "name": "GPT-6.1-Sol",
            "description": null,
            "isDefault": true,
            "effortLevels": ["low", "max", "ultra"]
        }]);
        assert_eq!(
            serde_json::to_value(catalog.known_models("codex").unwrap()).unwrap(),
            expected
        );
        std::fs::remove_file(executable).unwrap();
    }
}
