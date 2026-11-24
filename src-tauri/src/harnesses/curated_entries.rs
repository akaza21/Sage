//! The curated harness entries in catalog order, annotated with what
//! discovery, the published-release check and each binding report.

use super::{
    HarnessCatalog, SIMULATOR_ID, SIMULATOR_IN_CATALOG,
    catalog_definition::{self, CuratedProtocol, definition, safe_display_version},
    simulator_admission, version_details,
};
use crate::{acp::bindings::registry::ModelProvider, harness_discovery::known_harness_spec};
use sage_domain::harness::HarnessDefinition;

impl HarnessCatalog {
    fn registered_protocol(&self, harness_id: &str) -> CuratedProtocol<'_> {
        match self.registry.for_harness(harness_id) {
            Some(record) => match record.model_provider {
                ModelProvider::CodexAccount => CuratedProtocol::account_models(record.binding_id()),
                ModelProvider::ClaudeAdapter => CuratedProtocol::acp(Some(record.binding_id())),
                ModelProvider::NoAccountModels => CuratedProtocol::acp(Some(record.binding_id())),
            },
            None => CuratedProtocol::acp(None),
        }
    }

    pub fn definitions(&self) -> Vec<HarnessDefinition> {
        self.definitions_for(SIMULATOR_IN_CATALOG)
    }

    pub(super) fn definitions_for(&self, simulator_in_catalog: bool) -> Vec<HarnessDefinition> {
        let mut definitions = vec![
            definition(
                "codex",
                "Codex",
                "Launches the discovered local CLI through curated ACP integration.",
                self.codex_install_state().as_str(),
                "builtIn",
                self.registered_protocol("codex"),
            ),
            definition(
                "pi",
                "Pi",
                "Launches the discovered local Pi CLI over its RPC mode with read-only tools.",
                self.install_state("pi").as_str(),
                "builtIn",
                self.registered_protocol("pi"),
            ),
            definition(
                "gemini-cli",
                "Gemini CLI",
                "Discovers the local experimental ACP CLI; launch integration remains pending.",
                self.install_state("gemini-cli").as_str(),
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                "opencode",
                "OpenCode",
                "Launches the discovered local OpenCode CLI over its native ACP transport.",
                self.install_state("opencode").as_str(),
                "builtIn",
                self.registered_protocol("opencode"),
            ),
            definition(
                "cursor-agent",
                "Cursor Agent",
                "Discovers the local CLI; its Sage transport has not been characterised.",
                self.install_state("cursor-agent").as_str(),
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                "claude-code",
                "Claude Code",
                "Launches the discovered local Claude Code CLI over its Agent SDK protocol.",
                self.install_state("claude-code").as_str(),
                "builtIn",
                self.registered_protocol("claude-code"),
            ),
            definition(
                "oh-my-pi",
                "Oh My Pi",
                "Local CLI discovery; launch integration remains pending.",
                self.install_state("oh-my-pi").as_str(),
                "builtIn",
                CuratedProtocol::acp(None),
            ),
        ];
        definitions.extend(self.custom_acp_definitions());
        if let Some(install_state) = simulator_admission(simulator_in_catalog) {
            definitions.push(definition(
                SIMULATOR_ID,
                "Sage ACP Simulator",
                "Deterministic development harness for contract and recovery tests.",
                install_state.as_str(),
                "development",
                CuratedProtocol::legacy_simulator(),
            ));
        }
        #[cfg(all(windows, feature = "acp-fixture"))]
        if self.characterization_available && simulator_in_catalog {
            let simulator_position = definitions
                .iter()
                .position(|entry| entry.id == SIMULATOR_ID)
                .expect("curated catalog contains the simulator");
            definitions.insert(
                simulator_position,
                definition(
                    "sage-characterization",
                    "Sage Characterization Fixture",
                    "Exact fixture binding for disposable acceptance campaigns only.",
                    "available",
                    "development",
                    CuratedProtocol::acp(Some(crate::acp::service::CHARACTERIZATION_BINDING_ID)),
                ),
            );
            return self.annotate_discovery(definitions, simulator_in_catalog);
        }
        self.annotate_discovery(definitions, simulator_in_catalog)
    }

    fn annotate_discovery(
        &self,
        mut definitions: Vec<HarnessDefinition>,
        simulator_in_catalog: bool,
    ) -> Vec<HarnessDefinition> {
        let _ = simulator_in_catalog;
        for definition in &mut definitions {
            let spec = known_harness_spec(&definition.id);
            definition.brand = spec.and_then(|spec| spec.brand);
            definition.sign_in = spec.and_then(|spec| spec.sign_in).map(|sign_in| {
                sage_domain::harness::HarnessSignIn {
                    command: sign_in.command.into(),
                    hint: sign_in.hint.into(),
                    status_check: sign_in.status_check,
                }
            });
            if let Some(discovered) = self.discovered(&definition.id) {
                definition.version = discovered.version.as_deref().and_then(safe_display_version);
            }
            version_details::annotate(definition, spec, &self.published, &self.registry);
            definition.version_checking = self.discovered.is_version_checking(&definition.id);
            let Some(record) = self.registry.for_harness(&definition.id) else {
                continue;
            };
            if definition.id == "codex"
                && matches!(
                    record.auth_provider,
                    crate::acp::bindings::registry::AuthProvider::None
                )
            {
                definition.sign_in = None;
            }
            catalog_definition::annotate_binding(definition, record);
            self.annotate_custom_sign_in(definition);
        }
        definitions
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::by_id;
    use crate::{
        acp::bindings::codex::{CodexBindingRegistration, CodexHostPresence},
        harness_discovery::InstalledHarness,
        harnesses::{HarnessCatalog, SIMULATOR_ID},
        project_registry::create::ProjectCreateStore,
    };
    use sage_domain::harness::{HarnessInstallState, HarnessTransport};

    #[test]
    fn curated_harnesses_carry_their_brand_and_others_have_none() {
        use sage_domain::harness::HarnessBrand;
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        );
        let definitions = catalog.definitions();
        for (id, brand) in [
            ("claude-code", Some(HarnessBrand::Claude)),
            ("codex", Some(HarnessBrand::Codex)),
            ("opencode", Some(HarnessBrand::OpenCode)),
            ("pi", Some(HarnessBrand::Pi)),
            ("oh-my-pi", None),
            ("gemini-cli", None),
            ("cursor-agent", None),
        ] {
            assert_eq!(by_id(&definitions, id).brand, brand, "{id}");
        }
        let wire = serde_json::to_value(by_id(&definitions, "opencode")).unwrap();
        assert_eq!(wire["brand"], "opencode");
    }

    #[test]
    fn catalog_exposes_admission_order_and_explicit_transport_identity() {
        let definitions = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        )
        .settled_for_test()
        .definitions();
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.id.as_str())
                .collect::<Vec<_>>(),
            [
                "codex",
                "pi",
                "gemini-cli",
                "opencode",
                "cursor-agent",
                "claude-code",
                "oh-my-pi",
                SIMULATOR_ID,
            ]
        );
        assert_eq!(
            by_id(&definitions, "codex").transport,
            HarnessTransport::Acp
        );
        assert_eq!(
            by_id(&definitions, "codex").install_state,
            HarnessInstallState::NotInstalled.as_str()
        );
        assert_eq!(
            by_id(&definitions, "codex").binding_id.as_deref(),
            Some("codex-app-server")
        );
        for id in [
            "pi",
            "gemini-cli",
            "opencode",
            "cursor-agent",
            "claude-code",
            "oh-my-pi",
        ] {
            let entry = by_id(&definitions, id);
            assert_eq!(entry.transport, HarnessTransport::Acp);
            entry.validate_transport_contract().unwrap();
        }
        assert_eq!(
            by_id(&definitions, SIMULATOR_ID).transport,
            HarnessTransport::LegacySimulator
        );
        assert!(by_id(&definitions, SIMULATOR_ID).binding_id.is_none());
    }

    #[test]
    fn catalog_projects_an_absent_unbound_acp_harness_without_a_binding() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        )
        .settled_for_test();
        let definitions = catalog.definitions();
        let gemini = by_id(&definitions, "gemini-cli");
        assert_eq!(gemini.transport, HarnessTransport::Acp);
        assert!(gemini.binding_id.is_none());
        assert_eq!(
            gemini.install_state,
            HarnessInstallState::NotInstalled.as_str()
        );
    }

    #[test]
    fn transport_contract_rejects_ambiguous_or_invalid_bindings() {
        let mut definition = by_id(&HarnessCatalog::default().definitions(), "codex");
        definition.install_state = "available".into();
        definition.binding_id = None;
        assert!(definition.validate_transport_contract().is_err());
        definition.binding_id = Some(String::new());
        assert!(definition.validate_transport_contract().is_err());
        definition.binding_id = Some("invalid\0binding".into());
        assert!(definition.validate_transport_contract().is_err());
        definition.binding_id = Some("x".repeat(513));
        assert!(definition.validate_transport_contract().is_err());
        definition.transport = HarnessTransport::LegacySimulator;
        definition.binding_id = Some("codex-app-server".into());
        assert!(definition.validate_transport_contract().is_err());
    }

    #[test]
    fn transport_metadata_serializes_with_explicit_camel_case_fields() {
        let definitions = HarnessCatalog::default().definitions();
        let codex = serde_json::to_value(by_id(&definitions, "codex")).unwrap();
        assert_eq!(codex["transport"], "acp");
        assert_eq!(codex["bindingId"], "codex-app-server");
        assert_eq!(codex["accountModelDiscovery"], true);
        let simulator = serde_json::to_value(by_id(&definitions, SIMULATOR_ID)).unwrap();
        assert_eq!(simulator["transport"], "legacySimulator");
        assert!(simulator["bindingId"].is_null());
        assert_eq!(simulator["accountModelDiscovery"], false);
    }

    #[test]
    fn harness_bootstrap_projection_drops_native_paths_and_untrusted_version_prose() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        )
        .settled_for_test();
        let sentinel = std::env::temp_dir().join("private-inventory-sentinel");
        catalog.discovered.replace(
            [
                "codex",
                "pi",
                "gemini-cli",
                "opencode",
                "cursor-agent",
                "claude-code",
                "oh-my-pi",
            ]
            .into_iter()
            .map(|id| {
                InstalledHarness {
            id,
            executable: sentinel.join(id),
            version: Some(
                "C:\\private-inventory-sentinel\\cli.exe /home/private-inventory-sentinel 1.2.3"
                    .into(),
            ),
        }
            })
            .collect(),
        );
        let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
        let root =
            std::env::temp_dir().join(format!("sage-harness-projection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        database.create_project_with_chat(&root).unwrap();
        let mut state = crate::state::AppState::new(database);
        state.harnesses = catalog;
        let snapshot = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(crate::resource_commands::build_bootstrap_snapshot(
                &state, 0,
            ))
            .unwrap();
        let catalog = &state.harnesses;
        std::fs::remove_dir_all(root).unwrap();
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("private-inventory-sentinel"));
        assert!(!json.contains("executablePath"));
        for id in [
            "codex",
            "pi",
            "gemini-cli",
            "opencode",
            "cursor-agent",
            "claude-code",
            "oh-my-pi",
        ] {
            let entry = by_id(&snapshot.harnesses, id);
            assert_eq!(entry.install_state, "unavailable");
            assert_eq!(entry.version.as_deref(), Some("1.2.3"));
            let value = serde_json::to_value(&entry).unwrap();
            let keys: std::collections::BTreeSet<_> = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                [
                    "id",
                    "name",
                    "description",
                    "installState",
                    "source",
                    "transport",
                    "bindingId",
                    "accountModelDiscovery",
                    "accessModes",
                    "version",
                    "versionChecking",
                    "boundVersion",
                    "characterisation",
                    "unavailableReason",
                    "signIn",
                    "brand",
                    "update",
                    "latestVersion",
                    "runsVersion",
                    "directFromVersion"
                ]
                .into_iter()
                .collect()
            );
            assert!(!entry.version_checking);
            assert!(
                value
                    .as_object()
                    .unwrap()
                    .values()
                    .filter_map(serde_json::Value::as_str)
                    .all(|text| !text.contains(['/', '\\', ':']))
            );
            assert!(catalog.models(id).unwrap().is_empty());
        }
        assert!(!catalog.codex_registration().is_available());
        assert_eq!(
            by_id(&snapshot.harnesses, "claude-code")
                .binding_id
                .as_deref(),
            Some("claude-agent-acp")
        );
        assert_eq!(
            by_id(&snapshot.harnesses, "opencode").binding_id.as_deref(),
            Some("opencode-acp")
        );
        assert_eq!(
            by_id(&snapshot.harnesses, "pi").binding_id.as_deref(),
            Some("pi-rpc")
        );
        for id in ["gemini-cli", "cursor-agent", "oh-my-pi"] {
            assert!(by_id(&snapshot.harnesses, id).binding_id.is_none());
        }
    }

    #[test]
    fn catalog_projects_sign_in_guidance_from_known_harness_specs() {
        let definitions = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        )
        .definitions();
        let codex = by_id(&definitions, "codex");
        let sign_in = codex.sign_in.expect("Codex sign-in guidance");
        assert_eq!(sign_in.command, "codex login");
        assert_eq!(sign_in.hint, "Sign in to your Codex account.");
        let claude = by_id(&definitions, "claude-code");
        let value = serde_json::to_value(claude).expect("serialized catalog entry");
        assert_eq!(value["signIn"]["statusCheck"], "claudeCli");
        let pi = serde_json::to_value(by_id(&definitions, "pi")).expect("serialized Pi");
        assert!(pi["signIn"].get("statusCheck").is_none());
        let cursor = by_id(&definitions, "cursor-agent")
            .sign_in
            .expect("Cursor sign-in guidance");
        assert_eq!(cursor.command, "cursor-agent login");
        assert_eq!(cursor.hint, "Sign in to Cursor Agent.");
        assert!(by_id(&definitions, "oh-my-pi").sign_in.is_none());
    }

    #[test]
    fn development_policy_keeps_the_simulator_available() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Absent,
        );
        let simulator = by_id(&catalog.definitions_for(true), SIMULATOR_ID);
        assert_eq!(
            simulator.install_state,
            HarnessInstallState::Available.as_str()
        );
    }

    #[test]
    fn the_diagnostics_feature_never_admits_the_simulator_into_a_release_catalog() {
        assert_eq!(
            HarnessCatalog::default()
                .definitions()
                .iter()
                .any(|definition| definition.id == SIMULATOR_ID),
            cfg!(debug_assertions)
        );
    }
}
