//! Curated native binding metadata and admission authority, not runtime ownership.
//! AppState shares this registry with catalog and service. Gates, runtime slots,
//! schedulers, provider state and retained recovery remain service-global.

use std::sync::Arc;

use crate::{
    acp::{intent::CapabilitySnapshotV1, resource_mapper::ConfiguredResourceRoot},
    persistence::Database,
    state::ActiveProjectContext,
};

use super::{
    codex::{
        AdmittedCodexBinding, CODEX_BINDING_ID, CODEX_PROTOCOL_IDENTITY, CODEX_SCHEMA_SHA256,
        CodexAdmissionUnavailable, CodexBindingRegistration, CodexProjectRootAuthority,
        CodexRuntimeFactory, startup_codex_binding_registration,
    },
    runtime::RuntimeFactory,
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

/// Curated root selection is native metadata. Commands never choose a harness
/// root policy from an IPC request.
#[derive(Debug, Clone, Copy)]
enum RootPolicy {
    CodexSingleProject,
}

/// Opaque native root authority passed only from an admitted registry record to
/// its factory. The concrete authority stays inside curated native routing.
pub(crate) enum BindingProjectRoot {
    Codex(CodexProjectRootAuthority),
}

impl BindingProjectRoot {
    pub(crate) fn configured_resource_root(&self) -> ConfiguredResourceRoot {
        match self {
            Self::Codex(root) => ConfiguredResourceRoot {
                root_id: root.root_id().clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
        }
    }

    pub(crate) fn codex(&self) -> &CodexProjectRootAuthority {
        match self {
            Self::Codex(root) => root,
        }
    }
}

type LaunchFactory = fn(Arc<AdmittedCodexBinding>, BindingProjectRoot) -> Box<dyn RuntimeFactory>;
type SupportedCapabilities =
    fn(&AdmittedCodexBinding) -> Result<CapabilitySnapshotV1, CodexAdmissionUnavailable>;
type TurnDispatch =
    fn(
        &crate::acp::service::AcpService,
        crate::acp::store::PrepareTurnCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError>;
type CancelDispatch =
    fn(
        &crate::acp::service::AcpService,
        crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError>;
type CloseDispatch =
    fn(
        &crate::acp::service::AcpService,
        crate::acp::store::CloseSessionCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError>;

/// No Serialize or Debug: admission authority can contain native paths. This
/// slice registers only Codex in production; typed choices are curated metadata,
/// not plugins.
pub(crate) struct BindingRecord {
    harness_id: &'static str,
    binding_id: &'static str,
    admitted_binding_id: &'static str,
    wire: WireIdentity,
    root_policy: RootPolicy,
    admission: CodexBindingRegistration,
    launch_factory: LaunchFactory,
    supported_capabilities: SupportedCapabilities,
    turn_dispatch: TurnDispatch,
    cancel_dispatch: CancelDispatch,
    close_dispatch: CloseDispatch,
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

    pub(crate) fn dispatch_cancel(
        &self,
        service: &crate::acp::service::AcpService,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
        (self.cancel_dispatch)(service, intent)
    }

    pub(crate) fn dispatch_close(
        &self,
        service: &crate::acp::service::AcpService,
        command: crate::acp::store::CloseSessionCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
        (self.close_dispatch)(service, command)
    }

    fn admit(&self) -> Result<AdmittedBindingRecord<'_>, CodexAdmissionUnavailable> {
        let binding = self.admission.refresh()?;
        let metadata = binding.packet().metadata();
        if binding.binding_id() != self.admitted_binding_id
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
/// identity alone never yields a factory, including for an unavailable binding.
pub(crate) struct AdmittedBindingRecord<'a> {
    record: &'a BindingRecord,
    binding: Arc<AdmittedCodexBinding>,
}

impl AdmittedBindingRecord<'_> {
    pub(crate) fn binding_id(&self) -> &'static str {
        self.record.binding_id()
    }

    pub(crate) fn binding(&self) -> &Arc<AdmittedCodexBinding> {
        &self.binding
    }

    pub(crate) fn supported_capabilities(
        &self,
    ) -> Result<CapabilitySnapshotV1, CodexAdmissionUnavailable> {
        (self.record.supported_capabilities)(&self.binding)
    }

    pub(crate) fn project_root(
        &self,
        database: &Database,
        context: &ActiveProjectContext<'_>,
    ) -> Result<BindingProjectRoot, ()> {
        match self.record.root_policy {
            RootPolicy::CodexSingleProject => database
                .codex_project_root_authority(context)
                .map(BindingProjectRoot::Codex)
                .map_err(|_| ()),
        }
    }

    pub(crate) fn into_factory(self, root: BindingProjectRoot) -> Box<dyn RuntimeFactory> {
        (self.record.launch_factory)(self.binding, root)
    }

    pub(crate) fn dispatch_turn(
        &self,
        service: &crate::acp::service::AcpService,
        command: crate::acp::store::PrepareTurnCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
        (self.record.turn_dispatch)(service, command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryUnavailable {
    UnknownBinding,
    Admission(CodexAdmissionUnavailable),
}

pub(crate) struct BindingRegistry {
    codex: BindingRecord,
    #[cfg(test)]
    test_binding: Option<BindingRecord>,
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
                admitted_binding_id: CODEX_BINDING_ID,
                wire: WireIdentity {
                    protocol: CODEX_PROTOCOL_IDENTITY,
                    schema_sha256: CODEX_SCHEMA_SHA256,
                },
                root_policy: RootPolicy::CodexSingleProject,
                admission,
                launch_factory: codex_launch_factory,
                supported_capabilities: AdmittedCodexBinding::capability_ceiling,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::CodexAccount,
                auth_provider: AuthProvider::CodexHarness,
            },
            #[cfg(test)]
            test_binding: None,
        }
    }

    pub(crate) fn codex(&self) -> &BindingRecord {
        &self.codex
    }

    pub(crate) fn for_harness(&self, id: &str) -> Option<&BindingRecord> {
        if self.codex.harness_id() == id {
            return Some(&self.codex);
        }
        #[cfg(test)]
        if let Some(record) = self.test_binding.as_ref()
            && record.harness_id() == id
        {
            return Some(record);
        }
        None
    }

    pub(crate) fn resolve(
        &self,
        binding_id: &str,
    ) -> Result<AdmittedBindingRecord<'_>, RegistryUnavailable> {
        self.route(binding_id)?
            .admit()
            .map_err(RegistryUnavailable::Admission)
    }

    pub(crate) fn route(&self, binding_id: &str) -> Result<&BindingRecord, RegistryUnavailable> {
        self.record(binding_id)
            .ok_or(RegistryUnavailable::UnknownBinding)
    }

    fn record(&self, binding_id: &str) -> Option<&BindingRecord> {
        if self.codex.binding_id == binding_id {
            return Some(&self.codex);
        }
        #[cfg(test)]
        if let Some(record) = self.test_binding.as_ref()
            && record.binding_id == binding_id
        {
            return Some(record);
        }
        None
    }
}

fn codex_launch_factory(
    binding: Arc<AdmittedCodexBinding>,
    root: BindingProjectRoot,
) -> Box<dyn RuntimeFactory> {
    let BindingProjectRoot::Codex(root) = root;
    Box::new(CodexRuntimeFactory::from_admitted_binding(binding, root))
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
pub(crate) const TEST_BINDING_ID: &str = "registry-test-app-server";

#[cfg(test)]
static TEST_FACTORY_CONSTRUCTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static TEST_TURN_DISPATCHED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static TEST_CANCEL_DISPATCHED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static TEST_CLOSE_DISPATCHED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
            binding_id: TEST_BINDING_ID,
            admitted_binding_id: CODEX_BINDING_ID,
            wire: WireIdentity {
                protocol: CODEX_PROTOCOL_IDENTITY,
                schema_sha256: CODEX_SCHEMA_SHA256,
            },
            root_policy: RootPolicy::CodexSingleProject,
            admission: test_binding,
            launch_factory: test_launch_factory,
            supported_capabilities: AdmittedCodexBinding::capability_ceiling,
            turn_dispatch: test_turn_dispatch,
            cancel_dispatch: test_cancel_dispatch,
            close_dispatch: test_close_dispatch,
            model_provider: ModelProvider::CodexAccount,
            auth_provider: AuthProvider::CodexHarness,
        });
        registry
    }

    #[cfg(windows)]
    pub(crate) fn test_registration(&self) -> Option<CodexBindingRegistration> {
        self.test_binding
            .as_ref()
            .map(|record| record.admission().clone())
    }

    #[cfg(windows)]
    fn with_codex_and_wire(admission: CodexBindingRegistration, wire: WireIdentity) -> Self {
        let mut registry = Self::with_codex(admission);
        registry.codex.wire = wire;
        registry
    }
}

#[cfg(test)]
pub(crate) fn take_test_factory_construction_for_test() -> bool {
    TEST_FACTORY_CONSTRUCTED.swap(false, std::sync::atomic::Ordering::AcqRel)
}

#[cfg(test)]
pub(crate) fn take_test_turn_dispatch_for_test() -> bool {
    TEST_TURN_DISPATCHED.swap(false, std::sync::atomic::Ordering::AcqRel)
}

#[cfg(test)]
pub(crate) fn take_test_cancel_dispatch_for_test() -> bool {
    TEST_CANCEL_DISPATCHED.swap(false, std::sync::atomic::Ordering::AcqRel)
}

#[cfg(test)]
pub(crate) fn take_test_close_dispatch_for_test() -> bool {
    TEST_CLOSE_DISPATCHED.swap(false, std::sync::atomic::Ordering::AcqRel)
}

#[cfg(test)]
struct TestRuntimeFactory(CodexRuntimeFactory);

#[cfg(test)]
impl RuntimeFactory for TestRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: crate::acp::contract::RuntimeEpoch,
        session_id: crate::acp::contract::SageAgentSessionId,
    ) -> Result<super::runtime::NativeRuntime, super::adapter::AdapterRuntimeError> {
        TEST_FACTORY_CONSTRUCTED.store(true, std::sync::atomic::Ordering::Release);
        Box::new(self.0).construct(runtime_epoch, session_id)
    }
}

#[cfg(test)]
fn test_launch_factory(
    binding: Arc<AdmittedCodexBinding>,
    root: BindingProjectRoot,
) -> Box<dyn RuntimeFactory> {
    let BindingProjectRoot::Codex(root) = root;
    Box::new(TestRuntimeFactory(
        CodexRuntimeFactory::from_admitted_binding(binding, root),
    ))
}

#[cfg(test)]
fn test_turn_dispatch(
    _: &crate::acp::service::AcpService,
    _: crate::acp::store::PrepareTurnCommand,
) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
    TEST_TURN_DISPATCHED.store(true, std::sync::atomic::Ordering::Release);
    Err(crate::acp::service::CodexSessionError::Intent)
}

#[cfg(test)]
fn test_cancel_dispatch(
    _: &crate::acp::service::AcpService,
    _: crate::acp::runtime::HostCancelIntent,
) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
    TEST_CANCEL_DISPATCHED.store(true, std::sync::atomic::Ordering::Release);
    Err(crate::acp::service::CodexSessionError::Intent)
}

#[cfg(test)]
fn test_close_dispatch(
    _: &crate::acp::service::AcpService,
    _: crate::acp::store::CloseSessionCommand,
) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError> {
    TEST_CLOSE_DISPATCHED.store(true, std::sync::atomic::Ordering::Release);
    Err(crate::acp::service::CodexSessionError::Intent)
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

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
