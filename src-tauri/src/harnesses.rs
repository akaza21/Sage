use crate::{
    acp::bindings::codex::{
        CodexBindingRegistration, CodexHostPresence, observe_codex_host_presence,
        startup_codex_binding_registration,
    },
    domain::{HarnessDefinition, HarnessTransport, ModelOption},
    error::{AppError, AppResult},
    harness_discovery::{InstalledHarness, discover_installed_harnesses},
};

pub const SIMULATOR_ID: &str = "sage-simulator";
pub const SIMULATOR_DEFAULT_MODEL_ID: &str = "sim-fast";
pub const SIMULATOR_DEEP_MODEL_ID: &str = "sim-deep";

#[derive(Debug)]
pub struct HarnessCatalog {
    codex: CodexBindingRegistration,
    discovered: Vec<InstalledHarness>,
    #[cfg(test)]
    presence_override: Option<CodexHostPresence>,
    #[cfg(all(windows, feature = "acp-fixture"))]
    characterization_available: bool,
}

impl Default for HarnessCatalog {
    fn default() -> Self {
        Self::with_codex_registration(startup_codex_binding_registration())
    }
}

impl HarnessCatalog {
    pub(crate) fn with_codex_registration(codex: CodexBindingRegistration) -> Self {
        Self {
            codex,
            discovered: discover_installed_harnesses(),
            #[cfg(test)]
            presence_override: None,
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization_available: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_codex_registration_and_presence(
        codex: CodexBindingRegistration,
        presence: CodexHostPresence,
    ) -> Self {
        Self {
            codex,
            discovered: Vec::new(),
            presence_override: Some(presence),
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization_available: false,
        }
    }

    pub(crate) fn refresh_codex_registration(
        &self,
    ) -> Result<(), crate::acp::bindings::codex::CodexAdmissionUnavailable> {
        self.codex.refresh().map(drop)
    }

    #[cfg(test)]
    pub(crate) fn codex_registration(&self) -> CodexBindingRegistration {
        self.codex.clone()
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn with_characterization() -> Self {
        Self::with_codex_registration_and_characterization(startup_codex_binding_registration())
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn with_codex_registration_and_characterization(
        codex: CodexBindingRegistration,
    ) -> Self {
        Self {
            codex,
            discovered: discover_installed_harnesses(),
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

    fn codex_install_state(&self) -> &'static str {
        if self.codex.is_available() {
            "available"
        } else if self.discovered("codex").is_some() || self.observed_presence().is_present() {
            "unavailable"
        } else {
            "notInstalled"
        }
    }

    fn install_state(&self, harness_id: &str) -> &'static str {
        if self.discovered(harness_id).is_some() {
            "unavailable"
        } else {
            "notInstalled"
        }
    }

    fn discovered(&self, harness_id: &str) -> Option<&InstalledHarness> {
        self.discovered
            .iter()
            .find(|candidate| candidate.id == harness_id)
    }

    pub fn definitions(&self) -> Vec<HarnessDefinition> {
        let definitions = vec![
            definition(
                "codex",
                "Codex",
                "Launches the discovered local CLI through curated ACP integration.",
                self.codex_install_state(),
                "builtIn",
                CuratedProtocol::account_models("codex-app-server"),
            ),
            definition(
                "pi",
                "Pi",
                "Local CLI discovery; launch integration remains pending.",
                self.install_state("pi"),
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                "claude-code",
                "Claude Code",
                "Local CLI discovery; launch integration remains pending.",
                self.install_state("claude-code"),
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                "oh-my-pi",
                "Oh My Pi",
                "Local CLI discovery; launch integration remains pending.",
                self.install_state("oh-my-pi"),
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                SIMULATOR_ID,
                "Sage ACP Simulator",
                "Deterministic development harness for contract and recovery tests.",
                "available",
                "development",
                CuratedProtocol::legacy_simulator(),
            ),
        ];
        #[cfg(all(windows, feature = "acp-fixture"))]
        if self.characterization_available {
            let mut definitions = definitions;
            definitions.insert(
                4,
                definition(
                    "sage-characterization",
                    "Sage Characterization Fixture",
                    "Exact fixture binding for disposable acceptance campaigns only.",
                    "available",
                    "development",
                    CuratedProtocol::acp(Some(crate::acp::service::CHARACTERIZATION_BINDING_ID)),
                ),
            );
            return self.annotate_discovery(definitions);
        }
        self.annotate_discovery(definitions)
    }

    fn annotate_discovery(
        &self,
        mut definitions: Vec<HarnessDefinition>,
    ) -> Vec<HarnessDefinition> {
        for definition in &mut definitions {
            if let Some(discovered) = self.discovered(&definition.id) {
                definition.executable_path =
                    Some(discovered.executable.to_string_lossy().into_owned());
                definition.version = Some(discovered.version.clone());
            }
        }
        definitions
    }

    pub fn models(&self, harness_id: &str) -> AppResult<Vec<ModelOption>> {
        if harness_id == SIMULATOR_ID {
            return Ok(vec![
                ModelOption {
                    id: SIMULATOR_DEFAULT_MODEL_ID.into(),
                    name: "Simulator Fast".into(),
                    description: Some(
                        "Short deterministic responses for the walking skeleton.".into(),
                    ),
                    is_default: Some(true),
                },
                ModelOption {
                    id: SIMULATOR_DEEP_MODEL_ID.into(),
                    name: "Simulator Deep".into(),
                    description: Some("A second option for testing model changes.".into()),
                    is_default: Some(false),
                },
            ]);
        }

        let definition = self
            .definitions()
            .into_iter()
            .find(|definition| definition.id == harness_id)
            .ok_or_else(|| AppError::UnknownHarness(harness_id.into()))?;

        if definition.install_state != "available" || definition.transport == HarnessTransport::Acp
        {
            return Ok(Vec::new());
        }

        Err(AppError::HarnessUnavailable(harness_id.into()))
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

struct CuratedProtocol<'a> {
    transport: HarnessTransport,
    binding_id: Option<&'a str>,
    account_model_discovery: bool,
}

impl<'a> CuratedProtocol<'a> {
    fn account_models(binding_id: &'a str) -> Self {
        Self {
            transport: HarnessTransport::Acp,
            binding_id: Some(binding_id),
            account_model_discovery: true,
        }
    }

    fn acp(binding_id: Option<&'a str>) -> Self {
        Self {
            transport: HarnessTransport::Acp,
            binding_id,
            account_model_discovery: false,
        }
    }

    fn legacy_simulator() -> Self {
        Self {
            transport: HarnessTransport::LegacySimulator,
            binding_id: None,
            account_model_discovery: false,
        }
    }
}

fn definition(
    id: &str,
    name: &str,
    description: &str,
    install_state: &str,
    source: &str,
    protocol: CuratedProtocol<'_>,
) -> HarnessDefinition {
    let definition = HarnessDefinition {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        install_state: install_state.into(),
        source: source.into(),
        transport: protocol.transport,
        binding_id: protocol.binding_id.map(str::to_owned),
        account_model_discovery: protocol.account_model_discovery,
        executable_path: None,
        version: None,
    };
    definition
        .validate_transport_contract()
        .expect("curated harness metadata must satisfy its transport contract");
    definition
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_options_are_scoped_to_the_selected_harness() {
        let catalog = HarnessCatalog::default();
        assert_eq!(catalog.models(SIMULATOR_ID).unwrap().len(), 2);
        assert!(catalog.models("codex").unwrap().is_empty());
        assert!(catalog.models("oh-my-pi").unwrap().is_empty());
    }

    #[test]
    fn catalog_exposes_admission_order_and_explicit_transport_identity() {
        let definitions = HarnessCatalog::default().definitions();
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.id.as_str())
                .collect::<Vec<_>>(),
            ["codex", "pi", "claude-code", "oh-my-pi", SIMULATOR_ID]
        );
        assert_eq!(definitions[0].transport, HarnessTransport::Acp);
        assert!(matches!(
            definitions[0].install_state.as_str(),
            "notInstalled" | "unavailable" | "available"
        ));
        assert_eq!(
            definitions[0].binding_id.as_deref(),
            Some("codex-app-server")
        );
        assert!(definitions[1..4].iter().all(|definition| {
            definition.transport == HarnessTransport::Acp && definition.binding_id.is_none()
        }));
        assert_eq!(definitions[4].transport, HarnessTransport::LegacySimulator);
        assert!(definitions[4].binding_id.is_none());
    }

    #[test]
    fn transport_contract_rejects_ambiguous_or_invalid_bindings() {
        let mut definition = HarnessCatalog::default().definitions().remove(0);
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
        let codex = serde_json::to_value(&definitions[0]).unwrap();
        assert_eq!(codex["transport"], "acp");
        assert_eq!(codex["bindingId"], "codex-app-server");
        assert_eq!(codex["accountModelDiscovery"], true);
        let simulator = serde_json::to_value(&definitions[4]).unwrap();
        assert_eq!(simulator["transport"], "legacySimulator");
        assert!(simulator["bindingId"].is_null());
        assert_eq!(simulator["accountModelDiscovery"], false);
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
        );
        let codex = catalog.definitions().remove(0);
        assert_eq!(codex.install_state, "notInstalled");
        assert!(catalog.models("codex").unwrap().is_empty());
    }

    #[test]
    fn catalog_reports_present_unadmitted_codex_as_unavailable() {
        let catalog = HarnessCatalog::with_codex_registration_and_presence(
            crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test(),
            CodexHostPresence::Present,
        );
        let codex = catalog.definitions().remove(0);
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
        );
        let codex = catalog.definitions().remove(0);
        assert_eq!(codex.install_state, "available");
        assert_eq!(codex.binding_id.as_deref(), Some("codex-app-server"));
        assert!(catalog.models("codex").unwrap().is_empty());
    }
}
