use crate::{
    domain::{HarnessDefinition, HarnessTransport, ModelOption},
    error::{AppError, AppResult},
};

pub const SIMULATOR_ID: &str = "sage-simulator";
pub const SIMULATOR_DEFAULT_MODEL_ID: &str = "sim-fast";
pub const SIMULATOR_DEEP_MODEL_ID: &str = "sim-deep";

#[derive(Debug, Default)]
pub struct HarnessCatalog {
    #[cfg(all(windows, feature = "acp-fixture"))]
    characterization_available: bool,
}

impl HarnessCatalog {
    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn with_characterization() -> Self {
        Self {
            characterization_available: true,
        }
    }

    pub fn definitions(&self) -> Vec<HarnessDefinition> {
        let mut definitions = vec![
            definition(
                "codex",
                "Codex",
                "Curated ACP adapter, installed only when requested.",
                "notInstalled",
                "builtIn",
                HarnessTransport::Acp,
                Some("codex-app-server"),
            ),
            definition(
                "pi",
                "Pi",
                "Curated ACP adapter, installed only when requested.",
                "notInstalled",
                "builtIn",
                HarnessTransport::Acp,
                None,
            ),
            definition(
                "claude-code",
                "Claude Code",
                "Curated ACP adapter, installed only when requested.",
                "notInstalled",
                "builtIn",
                HarnessTransport::Acp,
                None,
            ),
            definition(
                "oh-my-pi",
                "Oh My Pi",
                "Curated native ACP binding, admitted last.",
                "notInstalled",
                "builtIn",
                HarnessTransport::Acp,
                None,
            ),
            definition(
                SIMULATOR_ID,
                "Sage ACP Simulator",
                "Deterministic development harness for contract and recovery tests.",
                "available",
                "development",
                HarnessTransport::LegacySimulator,
                None,
            ),
        ];
        #[cfg(all(windows, feature = "acp-fixture"))]
        if self.characterization_available {
            definitions.insert(
                4,
                definition(
                    "sage-characterization",
                    "Sage Characterization Fixture",
                    "Exact fixture binding for disposable acceptance campaigns only.",
                    "available",
                    "development",
                    HarnessTransport::Acp,
                    Some(crate::acp::service::CHARACTERIZATION_BINDING_ID),
                ),
            );
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
                },
                ModelOption {
                    id: SIMULATOR_DEEP_MODEL_ID.into(),
                    name: "Simulator Deep".into(),
                    description: Some("A second option for testing model changes.".into()),
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

fn definition(
    id: &str,
    name: &str,
    description: &str,
    install_state: &str,
    source: &str,
    transport: HarnessTransport,
    binding_id: Option<&str>,
) -> HarnessDefinition {
    let definition = HarnessDefinition {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        install_state: install_state.into(),
        source: source.into(),
        transport,
        binding_id: binding_id.map(str::to_owned),
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
        assert_eq!(definitions[0].install_state, "notInstalled");
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
        let simulator = serde_json::to_value(&definitions[4]).unwrap();
        assert_eq!(simulator["transport"], "legacySimulator");
        assert!(simulator["bindingId"].is_null());
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
}
