use crate::{
    acp::bindings::codex::{CodexBindingRegistration, startup_codex_binding_registration},
    domain::{HarnessDefinition, HarnessTransport, ModelOption},
    error::{AppError, AppResult},
};

pub const SIMULATOR_ID: &str = "sage-simulator";
pub const SIMULATOR_DEFAULT_MODEL_ID: &str = "sim-fast";
pub const SIMULATOR_DEEP_MODEL_ID: &str = "sim-deep";

#[derive(Debug)]
pub struct HarnessCatalog {
    codex: CodexBindingRegistration,
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
            characterization_available: true,
        }
    }

    pub fn definitions(&self) -> Vec<HarnessDefinition> {
        let codex_available = self.codex.is_available();
        let definitions = vec![
            definition(
                "codex",
                "Codex",
                "Curated ACP adapter, installed only when requested.",
                if codex_available {
                    "available"
                } else {
                    "notInstalled"
                },
                "builtIn",
                CuratedProtocol::account_models("codex-app-server"),
            ),
            definition(
                "pi",
                "Pi",
                "Curated ACP adapter, installed only when requested.",
                "notInstalled",
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                "claude-code",
                "Claude Code",
                "Curated ACP adapter, installed only when requested.",
                "notInstalled",
                "builtIn",
                CuratedProtocol::acp(None),
            ),
            definition(
                "oh-my-pi",
                "Oh My Pi",
                "Curated native ACP binding, admitted last.",
                "notInstalled",
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
            return definitions;
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
            "notInstalled" | "available"
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

    #[cfg(windows)]
    #[test]
    fn codex_availability_and_models_come_from_one_admitted_registration() {
        let unavailable = HarnessCatalog::with_codex_registration(
            crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test(),
        );
        let codex = unavailable.definitions().remove(0);
        assert_eq!(codex.install_state, "notInstalled");
        assert!(unavailable.models("codex").unwrap().is_empty());

        let registration =
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
        let admitted = HarnessCatalog::with_codex_registration(registration);
        let codex = admitted.definitions().remove(0);
        assert_eq!(codex.install_state, "available");
        assert_eq!(codex.binding_id.as_deref(), Some("codex-app-server"));
        assert!(admitted.models("codex").unwrap().is_empty());
    }
}
