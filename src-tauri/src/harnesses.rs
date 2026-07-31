use crate::{
    domain::{HarnessDefinition, ModelOption},
    error::{AppError, AppResult},
};

pub const SIMULATOR_ID: &str = "sage-simulator";

#[derive(Debug, Default)]
pub struct HarnessCatalog;

impl HarnessCatalog {
    pub fn definitions(&self) -> Vec<HarnessDefinition> {
        vec![
            HarnessDefinition {
                id: "claude-code".into(),
                name: "Claude Code".into(),
                description: "Curated ACP adapter, installed only when requested.".into(),
                install_state: "notInstalled".into(),
                source: "builtIn".into(),
            },
            HarnessDefinition {
                id: "codex".into(),
                name: "Codex".into(),
                description: "Curated ACP adapter, installed only when requested.".into(),
                install_state: "notInstalled".into(),
                source: "builtIn".into(),
            },
            HarnessDefinition {
                id: "pi".into(),
                name: "Pi".into(),
                description: "Curated ACP adapter, installed only when requested.".into(),
                install_state: "notInstalled".into(),
                source: "builtIn".into(),
            },
            HarnessDefinition {
                id: SIMULATOR_ID.into(),
                name: "Sage ACP Simulator".into(),
                description: "Deterministic development harness for contract and recovery tests."
                    .into(),
                install_state: "available".into(),
                source: "development".into(),
            },
        ]
    }

    pub fn models(&self, harness_id: &str) -> AppResult<Vec<ModelOption>> {
        if harness_id == SIMULATOR_ID {
            return Ok(vec![
                ModelOption {
                    id: "sim-fast".into(),
                    name: "Simulator Fast".into(),
                    description: Some(
                        "Short deterministic responses for the walking skeleton.".into(),
                    ),
                },
                ModelOption {
                    id: "sim-deep".into(),
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

        if definition.install_state != "available" {
            return Ok(Vec::new());
        }

        Err(AppError::HarnessUnavailable(harness_id.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_options_are_scoped_to_the_selected_harness() {
        let catalog = HarnessCatalog;
        assert_eq!(catalog.models(SIMULATOR_ID).unwrap().len(), 2);
        assert!(catalog.models("codex").unwrap().is_empty());
    }

    #[test]
    fn unknown_harness_is_rejected() {
        let catalog = HarnessCatalog;
        assert!(matches!(
            catalog.models("unknown"),
            Err(AppError::UnknownHarness(_))
        ));
    }
}
