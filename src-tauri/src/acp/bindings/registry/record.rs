use std::sync::Arc;

use crate::acp::{
    bindings::{
        claude::{AdmittedClaudeBinding, ClaudeBindingRegistration},
        codex::{AdmittedCodexBinding, CodexBindingRegistration, CodexProjectRootAuthority},
        runtime::RuntimeFactory,
    },
    intent::CapabilitySnapshotV1,
    resource_mapper::ConfiguredResourceRoot,
};

use super::{AuthProvider, ModelProvider, RegistryAdmissionUnavailable, WireIdentity};

#[derive(Debug, Clone, Copy)]
pub(super) enum RootPolicy {
    CodexSingleProject,
    ClaudeSingleProject,
}

pub(crate) enum BindingProjectRoot {
    Codex(CodexProjectRootAuthority),
    Claude(crate::project_registry::ProjectRootAuthority),
}

impl BindingProjectRoot {
    pub(crate) fn codex(&self) -> &CodexProjectRootAuthority {
        match self {
            Self::Codex(root) => root,
            Self::Claude(_) => panic!("Claude root cannot be used by Codex"),
        }
    }

    pub(crate) fn configured_resource_root(&self) -> ConfiguredResourceRoot {
        match self {
            Self::Codex(root) => ConfiguredResourceRoot {
                root_id: root.root_id().clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
            Self::Claude(root) => ConfiguredResourceRoot {
                root_id: root.scope().2.clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
        }
    }
}

type LaunchFactory = fn(AdmittedBinding, BindingProjectRoot) -> Box<dyn RuntimeFactory>;
type SupportedCapabilities =
    fn(&AdmittedBinding) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable>;
type TurnDispatch = fn(
    &crate::acp::service::AcpService,
    crate::acp::store::PrepareTurnCommand,
) -> Result<
    crate::acp::runtime::HostOutcome,
    crate::acp::service::PromptSendError<crate::acp::service::CodexSessionError>,
>;
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

pub(super) enum Admission {
    Codex(CodexBindingRegistration),
    Claude(Arc<ClaudeBindingRegistration>),
}

pub(super) enum AdmittedBinding {
    Codex(Arc<AdmittedCodexBinding>),
    Claude(Arc<AdmittedClaudeBinding>),
}

impl AdmittedBinding {
    fn binding_id(&self) -> &'static str {
        match self {
            Self::Codex(binding) => super::codex::binding_id(binding),
            Self::Claude(binding) => super::claude::binding_id(binding),
        }
    }
}

/// No Serialize or Debug: admission authority can contain native paths.
pub(crate) struct BindingRecord {
    pub(super) harness_id: &'static str,
    pub(super) binding_id: &'static str,
    pub(super) wire: Option<WireIdentity>,
    pub(super) root_policy: RootPolicy,
    pub(super) admission: Admission,
    pub(super) launch_factory: LaunchFactory,
    pub(super) supported_capabilities: SupportedCapabilities,
    pub(super) turn_dispatch: TurnDispatch,
    pub(super) cancel_dispatch: CancelDispatch,
    pub(super) close_dispatch: CloseDispatch,
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
        match &self.admission {
            Admission::Codex(admission) => admission,
            Admission::Claude(_) => panic!("Claude registration cannot be used as Codex admission"),
        }
    }

    pub(super) fn admit(&self) -> Result<AdmittedBindingRecord<'_>, RegistryAdmissionUnavailable> {
        let binding = match &self.admission {
            Admission::Codex(admission) => admission
                .refresh()
                .map(AdmittedBinding::Codex)
                .map_err(RegistryAdmissionUnavailable::Codex)?,
            Admission::Claude(admission) => admission
                .refresh()
                .map(AdmittedBinding::Claude)
                .map_err(RegistryAdmissionUnavailable::Claude)?,
        };
        if binding.binding_id() != self.binding_id
            || matches!((&binding, self.wire), (AdmittedBinding::Codex(_), None))
        {
            return Err(RegistryAdmissionUnavailable::Drift);
        }
        Ok(AdmittedBindingRecord {
            record: self,
            binding,
        })
    }

    pub(crate) fn admission_available(&self) -> bool {
        match &self.admission {
            Admission::Codex(admission) => admission.is_available(),
            Admission::Claude(admission) => admission.is_available(),
        }
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
}

pub(crate) struct AdmittedBindingRecord<'a> {
    record: &'a BindingRecord,
    binding: AdmittedBinding,
}

impl AdmittedBindingRecord<'_> {
    pub(crate) fn binding_id(&self) -> &'static str {
        self.record.binding_id()
    }

    pub(crate) fn binding(&self) -> &Arc<AdmittedCodexBinding> {
        super::codex::binding(&self.binding)
            .expect("Claude record cannot be consumed by Codex service")
    }

    pub(crate) fn project_root(
        &self,
        database: &crate::persistence::Database,
        context: &crate::state::ActiveProjectContext<'_>,
    ) -> Result<BindingProjectRoot, ()> {
        match self.record.root_policy {
            RootPolicy::CodexSingleProject => database
                .codex_project_root_authority(context)
                .map(BindingProjectRoot::Codex)
                .map_err(|_| ()),
            RootPolicy::ClaudeSingleProject => {
                let roots = database
                    .root_records(context.project_id().as_str())
                    .map_err(|_| ())?;
                let [root] = roots.as_slice() else {
                    return Err(());
                };
                let root_id =
                    crate::acp::contract::SageRootId::parse(root.id.clone()).map_err(|_| ())?;
                database
                    .project_root_authority(context, &root_id)
                    .map(BindingProjectRoot::Claude)
                    .map_err(|_| ())
            }
        }
    }

    pub(crate) fn into_factory(self, root: BindingProjectRoot) -> Box<dyn RuntimeFactory> {
        (self.record.launch_factory)(self.binding, root)
    }

    pub(crate) fn supported_capabilities(
        &self,
    ) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
        (self.record.supported_capabilities)(&self.binding)
    }

    pub(crate) fn dispatch_turn(
        &self,
        service: &crate::acp::service::AcpService,
        command: crate::acp::store::PrepareTurnCommand,
    ) -> Result<
        crate::acp::runtime::HostOutcome,
        crate::acp::service::PromptSendError<crate::acp::service::CodexSessionError>,
    > {
        (self.record.turn_dispatch)(service, command)
    }
}
