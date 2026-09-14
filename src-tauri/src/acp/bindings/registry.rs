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
