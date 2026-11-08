//! Curated native binding metadata and admission authority, not runtime ownership.

use std::sync::Arc;

use crate::acp::bindings::{
    claude::{CLAUDE_BINDING_ID, ClaudeAdmissionUnavailable, ClaudeBindingRegistration},
    codex::{
        CODEX_BINDING_ID, CODEX_PROTOCOL_IDENTITY, CODEX_SCHEMA_SHA256, CodexAdmissionUnavailable,
        CodexBindingRegistration, startup_codex_binding_registration,
    },
    standard_acp::{
        CustomAcpRegistration, CustomAcpUnavailable, OPENCODE_BINDING_ID,
        OpenCodeAdmissionUnavailable, OpenCodeBindingRegistration,
    },
};
use crate::settings_store::MAX_CUSTOM_ACP_AGENTS;

mod claude;
mod claude_sdk;
mod codex;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
mod pi;
mod pi_native;
mod record;
mod record_reason;
mod record_root;
mod standard_acp;
use record::{Admission, AdmittedBinding, RootPolicy};
pub(crate) use record::{AdmittedBindingRecord, BindingProjectRoot, BindingRecord};
#[cfg(all(test, windows))]
mod test_support;
#[cfg(all(test, windows))]
pub(crate) use test_support::{
    TEST_BINDING_ID, take_test_cancel_dispatch_for_test, take_test_close_dispatch_for_test,
    take_test_factory_construction_for_test, take_test_turn_dispatch_for_test,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ModelProvider {
    CodexAccount,
    ClaudeAdapter,
    NoAccountModels,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AuthProvider {
    Codex,
    Claude,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireIdentity {
    pub(crate) protocol: &'static str,
    pub(crate) schema_sha256: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RegistryAdmissionUnavailable {
    #[error(transparent)]
    Codex(CodexAdmissionUnavailable),
    #[error(transparent)]
    Claude(ClaudeAdmissionUnavailable),
    #[error(transparent)]
    OpenCode(OpenCodeAdmissionUnavailable),
    #[error("the user-defined ACP agent is not admitted: {0:?}")]
    CustomAcp(CustomAcpUnavailable),
    #[error("binding wire identity drifted after admission")]
    Drift,
}

impl From<RegistryAdmissionUnavailable> for CodexAdmissionUnavailable {
    fn from(value: RegistryAdmissionUnavailable) -> Self {
        match value {
            RegistryAdmissionUnavailable::Codex(reason) => reason,
            RegistryAdmissionUnavailable::Claude(_)
            | RegistryAdmissionUnavailable::OpenCode(_)
            | RegistryAdmissionUnavailable::CustomAcp(_)
            | RegistryAdmissionUnavailable::Drift => CodexAdmissionUnavailable::Drift,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryUnavailable {
    UnknownBinding,
    Admission(CodexAdmissionUnavailable),
}

pub(crate) struct BindingRegistry {
    codex: BindingRecord,
    claude: BindingRecord,
    claude_sdk: BindingRecord,
    opencode: BindingRecord,
    pi_native: BindingRecord,
    /// One fixed record per live user-defined agent.
    custom: Vec<BindingRecord>,
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pi: Option<BindingRecord>,
    #[cfg(test)]
    test_binding: Option<BindingRecord>,
}

impl Default for BindingRegistry {
    fn default() -> Self {
        Self::new(
            startup_codex_binding_registration(),
            ClaudeBindingRegistration::startup(),
        )
    }
}

impl BindingRegistry {
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
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

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn desktop_codex_fixture(
        selection: crate::launch_profile::CodexFixtureSelection,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        // A case whose session runs the model it is opened with offers its
        // catalogue through the same discovery a real Codex account uses.
        let offers_models = crate::acp::fixture::native_case_offers_models(&selection.case);
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

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn desktop_claude_fixture(
        selection: crate::launch_profile::FixtureSelection,
    ) -> Result<Self, ClaudeAdmissionUnavailable> {
        if crate::acp::fixture::native_case_matches(&selection.case, "claude-sdk") {
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

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
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

    pub(crate) fn new(codex: CodexBindingRegistration, claude: ClaudeBindingRegistration) -> Self {
        Self::new_with_opencode(codex, claude, OpenCodeBindingRegistration::startup())
    }

    fn new_with_opencode(
        codex: CodexBindingRegistration,
        claude: ClaudeBindingRegistration,
        opencode: OpenCodeBindingRegistration,
    ) -> Self {
        Self {
            codex: BindingRecord {
                harness_id: "codex",
                binding_id: CODEX_BINDING_ID,
                wire: Some(WireIdentity {
                    protocol: CODEX_PROTOCOL_IDENTITY,
                    schema_sha256: CODEX_SCHEMA_SHA256,
                }),
                root_policy: RootPolicy::Codex,
                session_operations_enabled: true,
                admission: Admission::Codex(codex),
                launch_factory: codex::launch_factory,
                supported_capabilities: codex::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::CodexAccount,
                auth_provider: AuthProvider::Codex,
            },
            claude: BindingRecord {
                harness_id: "claude-code",
                binding_id: CLAUDE_BINDING_ID,
                wire: None,
                root_policy: RootPolicy::Claude,
                session_operations_enabled: true,
                admission: Admission::Claude(Arc::new(claude)),
                launch_factory: claude::launch_factory,
                supported_capabilities: claude::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::ClaudeAdapter,
                auth_provider: AuthProvider::Claude,
            },
            claude_sdk: claude_sdk::record(Arc::new(
                crate::acp::bindings::claude::sdk::ClaudeSdkBindingRegistration::startup(),
            )),
            opencode: BindingRecord {
                harness_id: "opencode",
                binding_id: OPENCODE_BINDING_ID,
                wire: None,
                root_policy: RootPolicy::StandardAcp,
                session_operations_enabled: true,
                admission: Admission::OpenCode(Arc::new(opencode)),
                launch_factory: standard_acp::launch_factory,
                supported_capabilities: standard_acp::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::NoAccountModels,
                auth_provider: AuthProvider::None,
            },
            pi_native: pi_native::record(Arc::new(
                crate::acp::bindings::pi::native_admission::PiBindingRegistration::startup(),
            )),
            custom: (1..=MAX_CUSTOM_ACP_AGENTS)
                .filter_map(CustomAcpRegistration::startup)
                .map(|registration| standard_acp::custom_record(Arc::new(registration)))
                .collect(),
            #[cfg(test)]
            test_binding: None,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            pi: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_codex(codex: CodexBindingRegistration) -> Self {
        Self::new(codex, ClaudeBindingRegistration::absent_for_test())
    }

    pub(crate) fn codex(&self) -> &BindingRecord {
        &self.codex
    }

    pub(crate) fn codex_admission(&self) -> &CodexBindingRegistration {
        let Admission::Codex(admission) = &self.codex.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn claude_admission(&self) -> &Arc<ClaudeBindingRegistration> {
        let Admission::Claude(admission) = &self.claude.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn claude(&self) -> &BindingRecord {
        &self.claude
    }

    pub(crate) fn claude_sdk_admission(
        &self,
    ) -> &Arc<crate::acp::bindings::claude::sdk::ClaudeSdkBindingRegistration> {
        let Admission::ClaudeSdk(admission) = &self.claude_sdk.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn opencode(&self) -> &BindingRecord {
        &self.opencode
    }

    pub(crate) fn pi_admission(
        &self,
    ) -> &Arc<crate::acp::bindings::pi::native_admission::PiBindingRegistration> {
        let Admission::PiNative(admission) = &self.pi_native.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn opencode_admission(&self) -> &Arc<OpenCodeBindingRegistration> {
        let Admission::OpenCode(admission) = &self.opencode.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn for_harness(&self, id: &str) -> Option<&BindingRecord> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(record) = self.pi.as_ref().filter(|record| record.harness_id() == id) {
            return Some(record);
        }
        // Claude Code runs over its SDK protocol once that binding is
        // admitted; the ACP adapter serves only installs it cannot admit.
        let claude = if self.claude_sdk.admission_available() {
            &self.claude_sdk
        } else {
            self.claude()
        };
        [self.codex(), claude, self.opencode(), &self.pi_native]
            .into_iter()
            .chain(self.custom.iter())
            .find(|record| record.serves_harness(id))
            .or({
                #[cfg(test)]
                {
                    self.test_binding
                        .as_ref()
                        .filter(|record| record.harness_id() == id)
                }
                #[cfg(not(test))]
                {
                    None
                }
            })
    }

    pub(crate) fn resolve(
        &self,
        binding_id: &str,
    ) -> Result<AdmittedBindingRecord<'_>, RegistryUnavailable> {
        self.route(binding_id)?
            .admit()
            .map_err(|reason| RegistryUnavailable::Admission(reason.into()))
    }

    pub(crate) fn route(&self, binding_id: &str) -> Result<&BindingRecord, RegistryUnavailable> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(record) = self
            .pi
            .as_ref()
            .filter(|record| record.binding_id() == binding_id)
        {
            return Ok(record);
        }
        [
            &self.codex,
            &self.claude,
            &self.claude_sdk,
            &self.opencode,
            &self.pi_native,
        ]
        .into_iter()
        .chain(self.custom.iter())
        .find(|record| record.serves_binding(binding_id))
        .or({
            #[cfg(test)]
            {
                self.test_binding
                    .as_ref()
                    .filter(|record| record.binding_id() == binding_id)
            }
            #[cfg(not(test))]
            {
                None
            }
        })
        .ok_or(RegistryUnavailable::UnknownBinding)
    }
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BindingRegistry")
            .field("codex_available", &self.codex.admission_available())
            .field("claude_available", &self.claude.admission_available())
            .field("opencode_available", &self.opencode.admission_available())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl BindingRegistry {
    pub(crate) fn new_with_opencode_for_test(
        codex: CodexBindingRegistration,
        claude: ClaudeBindingRegistration,
        opencode: OpenCodeBindingRegistration,
    ) -> Self {
        Self::new_with_opencode(codex, claude, opencode)
    }

    #[cfg(windows)]
    pub(crate) fn with_test_binding(
        codex: CodexBindingRegistration,
        test_binding: CodexBindingRegistration,
    ) -> Self {
        let mut registry = Self::with_codex(codex);
        registry.test_binding = Some(BindingRecord {
            harness_id: "registry-test",
            binding_id: test_support::TEST_BINDING_ID,
            wire: Some(WireIdentity {
                protocol: CODEX_PROTOCOL_IDENTITY,
                schema_sha256: CODEX_SCHEMA_SHA256,
            }),
            root_policy: RootPolicy::Codex,
            session_operations_enabled: true,
            admission: Admission::Codex(test_binding),
            launch_factory: test_support::launch_factory,
            supported_capabilities: codex::capabilities,
            turn_dispatch: test_support::turn_dispatch,
            cancel_dispatch: test_support::cancel_dispatch,
            close_dispatch: test_support::close_dispatch,
            model_provider: ModelProvider::CodexAccount,
            auth_provider: AuthProvider::Codex,
        });
        registry
    }

    #[cfg(windows)]
    pub(crate) fn test_registration(&self) -> Option<CodexBindingRegistration> {
        match self.test_binding.as_ref()?.admission {
            Admission::Codex(ref admission) => Some(admission.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod pi_native_tests;
#[cfg(test)]
mod tests;
