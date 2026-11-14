use std::sync::Arc;

use super::{
    AuthProvider, BindingRegistry, ClaudeAdmissionUnavailable, ClaudeBindingRegistration,
    CodexAdmissionUnavailable, CodexBindingRegistration, ModelProvider, claude_sdk, pi,
};

impl BindingRegistry {
    pub(crate) fn desktop_pi_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, crate::acp::bindings::adapter::AdapterRuntimeError> {
        let binding =
            crate::acp::bindings::pi::fixture_admission::PiFixtureBinding::admit(selection)?;
        let mut registry = Self::new(
            CodexBindingRegistration::fixture_disabled(),
            ClaudeBindingRegistration::fixture_disabled(),
        );
        registry.isolate_fixture_records("pi");
        registry.pi = Some(pi::record(binding));
        Ok(registry)
    }

    pub(crate) fn desktop_codex_fixture(
        selection: crate::launch_profile::CodexFixtureSelection,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        // A case whose session runs the model it is opened with offers its
        // catalogue through the same discovery a real Codex account uses.
        let offers_models =
            crate::acp_protocol::native_cases::native_case_offers_models(&selection.case);
        let codex = CodexBindingRegistration::desktop_fixture(selection)?;
        let mut registry = Self::new(codex, ClaudeBindingRegistration::fixture_disabled());
        registry.codex.model_provider = if offers_models {
            ModelProvider::CodexAccount
        } else {
            ModelProvider::NoAccountModels
        };
        registry.codex.auth_provider = AuthProvider::None;
        registry.isolate_fixture_records(registry.codex.harness_id);
        Ok(registry)
    }

    pub(crate) fn desktop_claude_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, ClaudeAdmissionUnavailable> {
        if crate::acp_protocol::native_cases::native_case_matches(&selection.case, "claude-sdk") {
            let sdk = claude_sdk::fixture_record(selection)
                .map_err(|_| ClaudeAdmissionUnavailable::Drift)?;
            let mut registry = Self::new(
                CodexBindingRegistration::fixture_disabled(),
                ClaudeBindingRegistration::fixture_disabled(),
            );
            registry.isolate_fixture_records(sdk.harness_id);
            registry.claude_sdk = sdk;
            return Ok(registry);
        }
        let claude = ClaudeBindingRegistration::desktop_fixture(selection)?;
        let mut registry = Self::new(CodexBindingRegistration::fixture_disabled(), claude);
        registry.claude.model_provider = ModelProvider::NoAccountModels;
        registry.claude.auth_provider = AuthProvider::None;
        registry.isolate_fixture_records(registry.claude.harness_id);
        Ok(registry)
    }

    fn isolate_fixture_records(&mut self, active_harness: &str) {
        // Fixtures drive their own records; the SDK binding never shadows them.
        self.claude_sdk = claude_sdk::record(Arc::new(
            crate::acp::bindings::claude::sdk::ClaudeSdkBindingRegistration::fixture_disabled(),
        ));
        self.claude_sdk.session_operations_enabled = false;
        for record in [
            &mut self.codex,
            &mut self.claude,
            &mut self.opencode,
            &mut self.pi_native,
        ]
        .into_iter()
        .chain(self.custom.iter_mut())
        {
            if record.harness_id != active_harness {
                record.session_operations_enabled = false;
                record.model_provider = ModelProvider::NoAccountModels;
                record.auth_provider = AuthProvider::None;
            }
        }
    }
}
