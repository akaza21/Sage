//! Curated native binding metadata and admission authority, not runtime ownership.
//! AppState shares this registry with catalog and service. Gates, runtime slots,
//! schedulers, provider state and retained recovery remain service-global.

use std::sync::Arc;

use crate::acp::intent::CapabilitySnapshotV1;

use super::codex::{
    AdmittedCodexBinding, CODEX_BINDING_ID, CODEX_PROTOCOL_IDENTITY, CODEX_SCHEMA_SHA256,
    CodexAdmissionUnavailable, CodexBindingRegistration, CodexProjectRootAuthority,
    CodexRuntimeFactory, startup_codex_binding_registration,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ModelProvider {
    CodexAccount,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AuthProvider {
    CodexHarness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireIdentity {
    pub(crate) protocol: &'static str,
    pub(crate) schema_sha256: &'static str,
}

type LaunchFactory =
    fn(Arc<AdmittedCodexBinding>, CodexProjectRootAuthority) -> CodexRuntimeFactory;
type SupportedCapabilities =
    fn(&AdmittedCodexBinding) -> Result<CapabilitySnapshotV1, CodexAdmissionUnavailable>;

/// No Serialize or Debug: admission authority can contain native paths. This
/// slice registers only Codex; typed provider/factory choices are not plugins.
pub(crate) struct BindingRecord {
    harness_id: &'static str,
    binding_id: &'static str,
    wire: WireIdentity,
    admission: CodexBindingRegistration,
    launch_factory: LaunchFactory,
    supported_capabilities: SupportedCapabilities,
    pub(crate) model_provider: ModelProvider,
    pub(crate) auth_provider: AuthProvider,
}

impl BindingRecord {
    pub(crate) fn harness_id(&self) -> &'static str {
        self.harness_id
    }

    pub(crate) fn binding_id(&self) -> &'static str {
        self.binding_id
    }

    pub(crate) fn admission(&self) -> &CodexBindingRegistration {
        &self.admission
    }

    fn admit(&self) -> Result<AdmittedBindingRecord<'_>, CodexAdmissionUnavailable> {
        let binding = self.admission.refresh()?;
        let metadata = binding.packet().metadata();
        if binding.binding_id() != self.binding_id
            || metadata.capability_ceiling.protocol_identity != self.wire.protocol
            || metadata.app_server_schema_sha256 != self.wire.schema_sha256
        {
            self.admission.invalidate_drift();
            return Err(CodexAdmissionUnavailable::Drift);
        }
        Ok(AdmittedBindingRecord {
            record: self,
            binding,
        })
    }
}

/// Constructible only by refreshing the record's admission authority. Catalog
/// identity alone never yields a factory, including for an unavailable Codex.
pub(crate) struct AdmittedBindingRecord<'a> {
    record: &'a BindingRecord,
    binding: Arc<AdmittedCodexBinding>,
}

impl AdmittedBindingRecord<'_> {
    pub(crate) fn binding(&self) -> &Arc<AdmittedCodexBinding> {
        &self.binding
    }

    pub(crate) fn supported_capabilities(
        &self,
    ) -> Result<CapabilitySnapshotV1, CodexAdmissionUnavailable> {
        (self.record.supported_capabilities)(&self.binding)
    }

    pub(crate) fn into_factory(self, root: CodexProjectRootAuthority) -> CodexRuntimeFactory {
        (self.record.launch_factory)(self.binding, root)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryUnavailable {
    UnknownBinding,
    Admission(CodexAdmissionUnavailable),
}

pub(crate) struct BindingRegistry {
    codex: BindingRecord,
}

impl Default for BindingRegistry {
    fn default() -> Self {
        Self::with_codex(startup_codex_binding_registration())
    }
}

impl BindingRegistry {
    pub(crate) fn with_codex(admission: CodexBindingRegistration) -> Self {
        Self {
            codex: BindingRecord {
                harness_id: "codex",
                binding_id: CODEX_BINDING_ID,
                wire: WireIdentity {
                    protocol: CODEX_PROTOCOL_IDENTITY,
                    schema_sha256: CODEX_SCHEMA_SHA256,
                },
                admission,
                launch_factory: CodexRuntimeFactory::from_admitted_binding,
                supported_capabilities: AdmittedCodexBinding::capability_ceiling,
                model_provider: ModelProvider::CodexAccount,
                auth_provider: AuthProvider::CodexHarness,
            },
        }
    }

    pub(crate) fn codex(&self) -> &BindingRecord {
        &self.codex
    }

    pub(crate) fn for_harness(&self, id: &str) -> Option<&BindingRecord> {
        (self.codex.harness_id() == id).then_some(&self.codex)
    }

    pub(crate) fn resolve(
        &self,
        binding_id: &str,
    ) -> Result<AdmittedBindingRecord<'_>, RegistryUnavailable> {
        if self.codex.binding_id != binding_id {
            return Err(RegistryUnavailable::UnknownBinding);
        }
        self.codex.admit().map_err(RegistryUnavailable::Admission)
    }
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BindingRegistry")
            .field("binding_id", &self.codex.binding_id)
            .field("available", &self.codex.admission.is_available())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

    #[cfg(windows)]
    impl BindingRegistry {
        fn with_codex_and_wire(admission: CodexBindingRegistration, wire: WireIdentity) -> Self {
            let mut registry = Self::with_codex(admission);
            registry.codex.wire = wire;
            registry
        }
    }

    #[cfg(windows)]
    #[test]
    fn wire_mismatch_revokes_shared_admission_and_cannot_be_retried_as_valid() {
        use crate::{acp::service::AcpService, harnesses::HarnessCatalog, persistence::Database};

        for wire in [
            WireIdentity {
                protocol: "mismatched-protocol",
                schema_sha256: CODEX_SCHEMA_SHA256,
            },
            WireIdentity {
                protocol: CODEX_PROTOCOL_IDENTITY,
                schema_sha256: "mismatched-schema",
            },
        ] {
            let admission = CodexBindingRegistration::admitted_for_test();
            let valid = BindingRegistry::with_codex(admission.clone());
            let mismatched = Arc::new(BindingRegistry::with_codex_and_wire(
                admission.clone(),
                wire,
            ));
            let catalog = HarnessCatalog::with_registry(Arc::clone(&mismatched));
            let service = AcpService::new_with_registry_for_test(
                Database::open(std::path::Path::new(":memory:")).unwrap(),
                Arc::clone(&mismatched),
            )
            .unwrap();
            assert!(valid.resolve(CODEX_BINDING_ID).is_ok());
            assert!(admission.is_available());
            assert_eq!(
                catalog
                    .definitions()
                    .iter()
                    .find(|entry| entry.id == "codex")
                    .unwrap()
                    .install_state,
                "available"
            );

            assert!(matches!(
                mismatched.resolve(CODEX_BINDING_ID),
                Err(RegistryUnavailable::Admission(
                    CodexAdmissionUnavailable::Drift
                ))
            ));
            assert_eq!(
                admission.unavailable(),
                Some(CodexAdmissionUnavailable::Drift)
            );
            assert!(matches!(
                valid.resolve(CODEX_BINDING_ID),
                Err(RegistryUnavailable::Admission(
                    CodexAdmissionUnavailable::Drift
                ))
            ));
            assert!(!service.accepts_codex_binding(
                &crate::acp::intent::BindingId::new("bindingId", CODEX_BINDING_ID).unwrap()
            ));
            assert_ne!(
                catalog
                    .definitions()
                    .iter()
                    .find(|entry| entry.id == "codex")
                    .unwrap()
                    .install_state,
                "available"
            );
        }
    }
}
