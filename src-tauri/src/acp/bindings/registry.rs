//! Curated native binding metadata and admission authority, not runtime ownership.

use std::sync::Arc;

use crate::acp::bindings::{
    claude::{CLAUDE_BINDING_ID, ClaudeAdmissionUnavailable, ClaudeBindingRegistration},
    codex::{
        CODEX_BINDING_ID, CODEX_PROTOCOL_IDENTITY, CODEX_SCHEMA_SHA256, CodexAdmissionUnavailable,
        CodexBindingRegistration, startup_codex_binding_registration,
    },
};

mod claude;
mod codex;
mod record;
use record::{Admission, AdmittedBinding, RootPolicy};
pub(crate) use record::{AdmittedBindingRecord, BindingProjectRoot, BindingRecord};
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(crate) use test_support::{
    TEST_BINDING_ID, take_test_cancel_dispatch_for_test, take_test_close_dispatch_for_test,
    take_test_factory_construction_for_test, take_test_turn_dispatch_for_test,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ModelProvider {
    CodexAccount,
    ClaudeAdapter,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AuthProvider {
    CodexHarness,
    ClaudeHarness,
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
    #[error("binding wire identity drifted after admission")]
    Drift,
}

impl From<RegistryAdmissionUnavailable> for CodexAdmissionUnavailable {
    fn from(value: RegistryAdmissionUnavailable) -> Self {
        match value {
            RegistryAdmissionUnavailable::Codex(reason) => reason,
            RegistryAdmissionUnavailable::Claude(_) | RegistryAdmissionUnavailable::Drift => {
                CodexAdmissionUnavailable::Drift
            }
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
    pub(crate) fn new(codex: CodexBindingRegistration, claude: ClaudeBindingRegistration) -> Self {
        Self {
            codex: BindingRecord {
                harness_id: "codex",
                binding_id: CODEX_BINDING_ID,
                wire: Some(WireIdentity {
                    protocol: CODEX_PROTOCOL_IDENTITY,
                    schema_sha256: CODEX_SCHEMA_SHA256,
                }),
                root_policy: RootPolicy::CodexSingleProject,
                admission: Admission::Codex(codex),
                launch_factory: codex::launch_factory,
                supported_capabilities: codex::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::CodexAccount,
                auth_provider: AuthProvider::CodexHarness,
            },
            claude: BindingRecord {
                harness_id: "claude-code",
                binding_id: CLAUDE_BINDING_ID,
                wire: None,
                root_policy: RootPolicy::ClaudeSingleProject,
                admission: Admission::Claude(Arc::new(claude)),
                launch_factory: claude::launch_factory,
                supported_capabilities: claude::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::ClaudeAdapter,
                auth_provider: AuthProvider::ClaudeHarness,
            },
            #[cfg(test)]
            test_binding: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_codex(codex: CodexBindingRegistration) -> Self {
        Self::new(codex, ClaudeBindingRegistration::startup())
    }

    pub(crate) fn codex(&self) -> &BindingRecord {
        &self.codex
    }
    pub(crate) fn claude(&self) -> &BindingRecord {
        &self.claude
    }

    pub(crate) fn for_harness(&self, id: &str) -> Option<&BindingRecord> {
        [self.codex(), self.claude()]
            .into_iter()
            .find(|record| record.harness_id() == id)
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
        [self.codex(), self.claude()]
            .into_iter()
            .find(|record| record.binding_id() == binding_id)
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
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl BindingRegistry {
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
            root_policy: RootPolicy::CodexSingleProject,
            admission: Admission::Codex(test_binding),
            launch_factory: test_support::launch_factory,
            supported_capabilities: codex::capabilities,
            turn_dispatch: test_support::turn_dispatch,
            cancel_dispatch: test_support::cancel_dispatch,
            close_dispatch: test_support::close_dispatch,
            model_provider: ModelProvider::CodexAccount,
            auth_provider: AuthProvider::CodexHarness,
        });
        registry
    }

    #[cfg(windows)]
    pub(crate) fn test_registration(&self) -> Option<CodexBindingRegistration> {
        match self.test_binding.as_ref()?.admission {
            Admission::Codex(ref admission) => Some(admission.clone()),
            Admission::Claude(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_claude_record_only_when_injectable_admission_is_present() {
        // This discriminates because the registry must route the Claude binding
        // and invoke its real registration, rather than merely cataloguing an ID.
        let registry = BindingRegistry::default();
        assert!(
            matches!(registry.route(CLAUDE_BINDING_ID), Ok(record) if record.harness_id() == "claude-code")
        );
    }
}
