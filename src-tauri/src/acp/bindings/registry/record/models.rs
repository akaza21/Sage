use sage_domain::harness::ModelOption;

use super::{AdmittedBinding, AdmittedBindingRecord, RegistryAdmissionUnavailable};

impl AdmittedBindingRecord<'_> {
    pub(crate) fn launch_models(&self, probe: bool) -> Option<Vec<ModelOption>> {
        self.record.launch_models(probe)
    }

    pub(crate) fn accepts_launch_model(&self, model: &str) -> bool {
        self.launch_models(false)
            .is_some_and(|models| ModelOption::launched(&models, Some(model)).is_some())
    }

    pub(crate) fn launch_effort_choice(
        &self,
        model: &ModelOption,
        effort: &str,
    ) -> Option<sage_acp_protocol::session_config::ConfigMutation> {
        use sage_acp_protocol::session_config::{ConfigId, ConfigMutation, ConfigValue};
        let config_id = (self.record.launch_effort?)(model, effort)?;
        Some(ConfigMutation::Option {
            config_id: ConfigId::new("configId", config_id).ok()?,
            value: ConfigValue::Select {
                value: ConfigId::new("value", effort).ok()?,
            },
        })
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
