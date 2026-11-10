use std::sync::Arc;

mod catalog_definition;
mod claude_auth;
mod custom_acp;
mod discovery_state;
mod install_state;
mod persisted_binding;
use catalog_definition::{CuratedProtocol, definition};
use discovery_state::DiscoveryState;

use crate::{
    acp::bindings::{
        codex::{CodexHostPresence, observe_codex_host_presence},
        registry::{BindingRegistry, ModelProvider},
    },
    domain::{HarnessDefinition, HarnessInstallState, HarnessTransport, ModelOption},
    error::{AppError, AppResult},
    harness_discovery::{InstalledHarness, known_harness_spec},
};

#[cfg(test)]
use crate::domain::HarnessUnavailableReason;

#[cfg(test)]
use crate::acp::bindings::codex::CodexBindingRegistration;
#[cfg(all(test, windows, feature = "acp-fixture"))]
use crate::acp::bindings::codex::startup_codex_binding_registration;

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

impl HarnessCatalog {
    pub(crate) fn with_registry(registry: Arc<BindingRegistry>) -> Self {
        Self {
            registry,
            discovered: DiscoveryState::default(),
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

    pub(crate) fn apply_presence(&self, discovered: Vec<InstalledHarness>) {
        self.discovered.apply_presence(discovered);
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
            #[cfg(test)]
            presence_override: None,
            characterization_available: true,
        }
    }

    fn observed_presence(&self) -> CodexHostPresence {
        #[cfg(test)]
        if let Some(presence) = self.presence_override {
            return presence;
        }
        observe_codex_host_presence()
    }

    fn discovered(&self, harness_id: &str) -> Option<InstalledHarness> {
        self.discovered.find(harness_id)
    }

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

    fn definitions_for(&self, simulator_in_catalog: bool) -> Vec<HarnessDefinition> {
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
            definition.sign_in =
                spec.and_then(|spec| spec.sign_in)
                    .map(|sign_in| crate::domain::HarnessSignIn {
                        command: sign_in.command.into(),
                        hint: sign_in.hint.into(),
                    });
            if let Some(discovered) = self.discovered(&definition.id) {
                definition.version = discovered.version.as_deref().and_then(safe_display_version);
            }
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

    pub fn models(&self, harness_id: &str) -> AppResult<Vec<ModelOption>> {
        self.models_for(harness_id, SIMULATOR_IN_CATALOG)
    }

    fn models_for(
        &self,
        harness_id: &str,
        simulator_in_catalog: bool,
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
                },
                ModelOption {
                    id: SIMULATOR_DEEP_MODEL_ID.into(),
                    name: "Simulator Deep".into(),
                    description: Some("A second option for testing model changes.".into()),
                    is_default: Some(false),
                    context_window: None,
                },
            ]);
        }

        let definition = self
            .definitions_for(simulator_in_catalog)
            .into_iter()
            .find(|definition| definition.id == harness_id)
            .ok_or_else(|| AppError::UnknownHarness(harness_id.into()))?;

        // Claude Code lists its models only once a session runs; until then
        // the picker offers the aliases a session can start with.
        if definition.install_state == "available"
            && definition.binding_id.as_deref()
                == Some(crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID)
        {
            return Ok(crate::acp::bindings::claude::sdk::launch_models());
        }
        if definition.install_state != "available" || definition.transport == HarnessTransport::Acp
        {
            return Ok(Vec::new());
        }

        Err(AppError::HarnessUnavailable(harness_id.into()))
    }

    pub fn default_selected_harness_id(&self) -> Option<String> {
        self.default_selected_harness_id_for(SIMULATOR_IN_CATALOG)
    }

    fn default_selected_harness_id_for(&self, simulator_in_catalog: bool) -> Option<String> {
        let definitions = self.definitions_for(simulator_in_catalog);
        definitions
            .iter()
            .find(|definition| {
                definition.install_state == "available" && definition.source != "development"
            })
            .or_else(|| {
                definitions
                    .iter()
                    .find(|definition| definition.install_state == "available")
            })
            .map(|definition| definition.id.clone())
    }

    pub fn harness_id_for_binding(&self, binding_id: &str) -> Option<String> {
        #[cfg(all(windows, feature = "acp-fixture"))]
        if self.characterization_available
            && binding_id == crate::acp::service::CHARACTERIZATION_BINDING_ID
        {
            return Some("sage-characterization".into());
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

/// Untrusted CLI version prose may contain paths or secrets. Project only a
/// bounded dotted numeric token, never the executable identity or raw output.
fn safe_display_version(version: &str) -> Option<String> {
    version.split_ascii_whitespace().find_map(|token| {
        let token = token.strip_prefix('v').unwrap_or(token);
        let parts: Vec<_> = token.split('.').collect();
        let numeric = token.len() <= 32
            && (2..=4).contains(&parts.len())
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.bytes().all(|byte| byte.is_ascii_digit())
            });
        if numeric {
            return Some(token.to_owned());
        }
        let (base, prerelease) = token.split_once('-')?;
        let base_parts: Vec<_> = base.split('.').collect();
        (token.len() <= 96
            && base_parts.len() == 3
            && base_parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.bytes().all(|byte| byte.is_ascii_digit())
            })
            && !prerelease.is_empty()
            && prerelease.len() <= 64
            && prerelease
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-'))
        .then(|| token.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::fs;

    fn by_id(definitions: &[HarnessDefinition], id: &str) -> HarnessDefinition {
        definitions
            .iter()
            .find(|definition| definition.id == id)
            .unwrap()
            .clone()
    }

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
    fn curated_harnesses_carry_their_brand_and_others_have_none() {
        use crate::domain::HarnessBrand;
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
            Some(crate::domain::HarnessCharacterisation::Characterised)
        );
        assert!(opencode.unavailable_reason.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn an_uncharacterised_opencode_build_never_borrows_the_characterised_claims() {
        let root = std::env::temp_dir().join(format!(
            "sage-opencode-uncharacterised-{}",
            uuid::Uuid::new_v4()
        ));
        let catalog = admitted_opencode_catalog(&root, "1.19.0");
        let opencode = by_id(&catalog.definitions(), "opencode");
        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            opencode.install_state,
            HarnessInstallState::Available.as_str()
        );
        assert_eq!(opencode.binding_id.as_deref(), Some("opencode-acp"));
        assert_eq!(opencode.bound_version.as_deref(), Some("1.19.0"));
        assert_eq!(
            opencode.characterisation,
            Some(crate::domain::HarnessCharacterisation::Uncharacterised)
        );
        assert!(!opencode.account_model_discovery);
    }

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
        };
        assert_eq!(
            serde_json::to_value(known).unwrap()["contextWindow"],
            272_000
        );
        assert!(catalog.models("codex").unwrap().is_empty());
        assert!(catalog.models("oh-my-pi").unwrap().is_empty());
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
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let root =
            std::env::temp_dir().join(format!("sage-harness-projection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        database.create_project_from_root(&root).unwrap();
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
                    "brand"
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
    fn harness_display_version_never_echoes_paths_or_control_characters() {
        assert_eq!(
            safe_display_version("codex-cli v1.2.3"),
            Some("1.2.3".into())
        );
        assert_eq!(
            safe_display_version("codex-cli 0.155.0-alpha.16.3"),
            Some("0.155.0-alpha.16.3".into())
        );
        for text in [
            "C:\\private\\1.2.exe",
            "/home/private/1.2",
            "https://host/1.2",
            "1.2\u{1b}",
            "1.123456789",
            "",
            "private label",
            "0.155.0-",
            "0.155.0-alpha/secret",
        ] {
            assert_eq!(safe_display_version(text), None, "{text}");
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
        let cursor = by_id(&definitions, "cursor-agent")
            .sign_in
            .expect("Cursor sign-in guidance");
        assert_eq!(cursor.command, "cursor-agent login");
        assert_eq!(cursor.hint, "Sign in to Cursor Agent.");
        assert!(by_id(&definitions, "oh-my-pi").sign_in.is_none());
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
            Some(crate::domain::HarnessUnavailableReason::AdapterAbsent),
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
            catalog.models_for(SIMULATOR_ID, false),
            Err(AppError::UnknownHarness(_))
        ));
        assert_ne!(
            catalog.default_selected_harness_id_for(false).as_deref(),
            Some(SIMULATOR_ID)
        );
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

    #[cfg(windows)]
    #[test]
    fn development_default_prefers_an_available_curated_harness() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            CodexBindingRegistration::admitted_for_test(),
            CodexHostPresence::Present,
        )
        .settled_for_test();

        assert_eq!(
            catalog.default_selected_harness_id_for(true).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn configured_catalog_matches_the_current_build_policy() {
        let definitions = HarnessCatalog::default().definitions();
        let default_id = HarnessCatalog::default().default_selected_harness_id();
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
    fn the_diagnostics_feature_never_admits_the_simulator_into_a_release_catalog() {
        assert_eq!(SIMULATOR_IN_CATALOG, cfg!(debug_assertions));
        assert_eq!(
            HarnessCatalog::default()
                .definitions()
                .iter()
                .any(|definition| definition.id == SIMULATOR_ID),
            cfg!(debug_assertions)
        );
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
                adapter_version: "0.99.0".into(),
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
            Some(crate::domain::HarnessCharacterisation::Uncharacterised)
        );
        assert_eq!(entry.bound_version.as_deref(), Some("0.99.0"));
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
