use sage_acp_protocol::session_config::{ConfigId, ConfigMutation, ConfigOption, ConfigValue};
use sage_domain::harness::ModelOption;

use super::{AdmittedBinding, AdmittedBindingRecord, RegistryAdmissionUnavailable};

/// A thinking level a session starts with: the choice stored for it, and the
/// option the binding offers it through, with the other levels it lists.
pub(crate) struct LaunchEffortChoice {
    pub(crate) choice: ConfigMutation,
    pub(crate) offered: ConfigOption,
}

impl AdmittedBindingRecord<'_> {
    pub(crate) fn launch_models(&self, probe: bool) -> Option<Vec<ModelOption>> {
        self.record.launch_models(probe)
    }

    pub(crate) fn accepts_launch_model(&self, model: &str) -> bool {
        self.launch_models(false)
            .is_some_and(|models| ModelOption::launched(&models, Some(model)).is_some())
    }

    /// How a session started on the listed `model` takes thinking level
    /// `effort`; `None` when the binding does not offer that level for it.
    pub(crate) fn launch_effort_choice(
        &self,
        model: &ModelOption,
        effort: &str,
    ) -> Option<LaunchEffortChoice> {
        let offered = (self.record.launch_effort?)(model, effort)?;
        let choice = ConfigMutation::Option {
            config_id: offered.id.clone(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", effort).ok()?,
            },
        };
        Some(LaunchEffortChoice { choice, offered })
    }

    pub(crate) fn has_model_discovery(&self) -> Result<bool, RegistryAdmissionUnavailable> {
        match (&self.binding, self.record.model_provider) {
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            (AdmittedBinding::Pi(_), super::ModelProvider::NoAccountModels) => Ok(false),
            (AdmittedBinding::Codex(binding), super::ModelProvider::CodexAccount) => binding
                .model_discovery_available()
                .map_err(RegistryAdmissionUnavailable::Codex),
            (AdmittedBinding::Codex(binding), super::ModelProvider::NoAccountModels)
                if binding.is_desktop_fixture() =>
            {
                Ok(false)
            }
            (
                AdmittedBinding::Claude(_) | AdmittedBinding::ClaudeSdk(_),
                super::ModelProvider::ClaudeAdapter,
            ) => Ok(false),
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            (AdmittedBinding::Claude(binding), super::ModelProvider::NoAccountModels)
                if binding.is_fixture() =>
            {
                Ok(false)
            }
            (AdmittedBinding::StandardAcp { .. }, super::ModelProvider::NoAccountModels) => {
                Ok(false)
            }
            (AdmittedBinding::PiNative(_), super::ModelProvider::NoAccountModels) => Ok(false),
            _ => Err(RegistryAdmissionUnavailable::Drift),
        }
    }
}
