use std::sync::Arc;

use crate::acp::{
    bindings::{
        claude::{AdmittedClaudeBinding, ClaudeBindingRegistration},
        codex::{AdmittedCodexBinding, CodexBindingRegistration, CodexProjectRootAuthority},
        runtime::RuntimeFactory,
        standard_acp::{AdmittedStandardAcpBinding, OpenCodeBindingRegistration},
    },
    intent::CapabilitySnapshotV1,
    resource_mapper::ConfiguredResourceRoot,
};
use crate::domain::{HarnessCharacterisation, HarnessUnavailableReason};

use super::{AuthProvider, ModelProvider, RegistryAdmissionUnavailable, WireIdentity};

#[derive(Debug, Clone, Copy)]
pub(super) enum RootPolicy {
    Codex,
    Claude,
    OpenCode,
}

pub(crate) enum BindingProjectRoot {
    Codex(CodexProjectRootAuthority),
    Claude(crate::project_registry::ProjectRootAuthority),
    OpenCode(crate::project_registry::ProjectRootAuthority),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BindingProjectRootError {
    #[error("the admitted project root no longer revalidates")]
    Revalidation,
    #[error("the admitted project root is not valid UTF-8")]
    NonUnicode,
    #[error("the admitted project root is empty")]
    Empty,
}

impl BindingProjectRoot {
    pub(crate) fn project_matches(&self, project_id: &crate::acp::contract::SageProjectId) -> bool {
        match self {
            Self::Codex(root) => root.project_id() == project_id,
            Self::Claude(root) => root.scope().0 == project_id,
            Self::OpenCode(root) => root.scope().0 == project_id,
        }
    }

    pub(crate) fn revalidate(&self) -> Result<(), BindingProjectRootError> {
        match self {
            Self::Codex(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
            Self::Claude(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
            Self::OpenCode(root) => root
                .revalidate()
                .map_err(|_| BindingProjectRootError::Revalidation),
        }
    }

    pub(crate) fn native_root(&self) -> Result<String, BindingProjectRootError> {
        let path = match self {
            Self::Codex(root) => root.as_path(),
            Self::Claude(root) => root.as_path(),
            Self::OpenCode(root) => root.as_path(),
        };
        let path = path.to_str().ok_or(BindingProjectRootError::NonUnicode)?;
        let path = path
            .strip_prefix(r"\\?\")
            .or_else(|| path.strip_prefix(r"//?/"))
            .unwrap_or(path);
        (!path.is_empty())
            .then(|| path.to_owned())
            .ok_or(BindingProjectRootError::Empty)
    }

    pub(crate) fn codex_project_root(
        &self,
    ) -> Result<&CodexProjectRootAuthority, RegistryAdmissionUnavailable> {
        match self {
            Self::Codex(root) => Ok(root),
            Self::Claude(_) | Self::OpenCode(_) => Err(RegistryAdmissionUnavailable::Drift),
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
            Self::OpenCode(root) => ConfiguredResourceRoot {
                root_id: root.scope().2.clone(),
                canonical_native_root: root.as_path().to_owned(),
            },
        }
    }
}

type LaunchFactory = fn(
    AdmittedBinding,
    BindingProjectRoot,
    crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory>;
type SupportedCapabilities =
    fn(&AdmittedBinding) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable>;
type TurnDispatch = fn(
    &crate::acp::service::AcpService,
    crate::acp::runtime::HostPromptIntent,
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
    OpenCode(Arc<OpenCodeBindingRegistration>),
}

pub(crate) struct BoundBuild {
    pub(crate) reported_version: String,
    pub(crate) characterisation: HarnessCharacterisation,
    pub(crate) account_model_discovery: bool,
}

pub(super) enum AdmittedBinding {
    Codex(Arc<AdmittedCodexBinding>),
    Claude(Arc<AdmittedClaudeBinding>),
    OpenCode {
        registration: Arc<OpenCodeBindingRegistration>,
        binding: Arc<AdmittedStandardAcpBinding>,
    },
}

/// No Serialize or Debug: admission authority can contain native paths.
pub(crate) struct BindingRecord {
    pub(super) harness_id: &'static str,
    pub(super) binding_id: &'static str,
    pub(super) wire: Option<WireIdentity>,
    pub(super) root_policy: RootPolicy,
    pub(super) session_operations_enabled: bool,
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

    pub(crate) fn session_operations_enabled(&self) -> bool {
        self.session_operations_enabled
    }

    pub(crate) fn admit(&self) -> Result<AdmittedBindingRecord<'_>, RegistryAdmissionUnavailable> {
        let binding = match &self.admission {
            Admission::Codex(admission) => admission
                .refresh()
                .map(AdmittedBinding::Codex)
                .map_err(RegistryAdmissionUnavailable::Codex)?,
            Admission::Claude(admission) => admission
                .refresh()
                .map(AdmittedBinding::Claude)
                .map_err(RegistryAdmissionUnavailable::Claude)?,
            Admission::OpenCode(admission) => admission
                .refresh()
                .map(|binding| AdmittedBinding::OpenCode {
                    registration: Arc::clone(admission),
                    binding,
                })
                .map_err(RegistryAdmissionUnavailable::OpenCode)?,
        };
        if matches!((&binding, self.wire), (AdmittedBinding::Codex(_), None)) {
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
            Admission::OpenCode(admission) => admission.is_available(),
        }
    }

    pub(crate) fn bound_build(&self) -> Option<BoundBuild> {
        match &self.admission {
            Admission::Codex(admission) => admission.admitted().map(|binding| BoundBuild {
                reported_version: binding.harness_version().to_owned(),
                characterisation: binding.characterisation(),
                account_model_discovery: binding.model_discovery_available().unwrap_or(false),
            }),
            Admission::Claude(admission) => admission.admitted().map(|binding| BoundBuild {
                reported_version: binding.harness_version().to_owned(),
                characterisation: binding.characterisation(),
                account_model_discovery: false,
            }),
            Admission::OpenCode(admission) => admission.admitted().map(|binding| BoundBuild {
                reported_version: binding.harness_version().to_owned(),
                characterisation: crate::acp::bindings::characterisation::opencode_characterisation(
                    binding.harness_version(),
                ),
                account_model_discovery: false,
            }),
        }
    }

    pub(crate) fn unavailable_reason(&self) -> Option<HarnessUnavailableReason> {
        use crate::acp::bindings::claude::ClaudeAdmissionUnavailable as Claude;
        use crate::acp::bindings::codex::CodexAdmissionUnavailable as Codex;
        use crate::acp::bindings::standard_acp::OpenCodeAdmissionUnavailable as OpenCode;
        match &self.admission {
            Admission::Codex(admission) => admission.unavailable().map(|reason| match reason {
                Codex::Platform => HarnessUnavailableReason::PlatformUnsupported,
                Codex::ObservationOnly => HarnessUnavailableReason::ObservationOnly,
                Codex::IndependentAdmission => HarnessUnavailableReason::AdmissionIncomplete,
                Codex::Drift => HarnessUnavailableReason::DriftDetected,
                Codex::Projection(_) => HarnessUnavailableReason::CapabilityProjectionUnavailable,
            }),
            Admission::Claude(admission) => admission.unavailable().map(|reason| match reason {
                Claude::DiscoveryPending => HarnessUnavailableReason::AdmissionIncomplete,
                Claude::AdapterAbsent => HarnessUnavailableReason::AdapterAbsent,
                Claude::AdapterVersionUnreadable => {
                    HarnessUnavailableReason::AdapterVersionUnreadable
                }
                Claude::NodeAbsent => HarnessUnavailableReason::NodeAbsent,
                Claude::NodeVersionUnreadable => HarnessUnavailableReason::NodeVersionUnreadable,
                Claude::NodeEngineMismatch => HarnessUnavailableReason::NodeEngineMismatch,
                Claude::CandidateInvalid => HarnessUnavailableReason::CandidateInvalid,
                Claude::Drift => HarnessUnavailableReason::DriftDetected,
            }),
            Admission::OpenCode(admission) => admission.unavailable().map(|reason| match reason {
                OpenCode::DiscoveryPending => HarnessUnavailableReason::AdmissionIncomplete,
                OpenCode::Absent => HarnessUnavailableReason::CandidateInvalid,
                OpenCode::CandidateInvalid => HarnessUnavailableReason::CandidateInvalid,
                OpenCode::Drift => HarnessUnavailableReason::DriftDetected,
            }),
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

pub(crate) struct BindingIdentity<'a> {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) binding_digest_hex: &'a str,
    pub(crate) artifact_digest: &'a str,
    pub(crate) harness_version: &'a str,
}

impl AdmittedBindingRecord<'_> {
    pub(crate) fn binding_id(&self) -> &'static str {
        self.record.binding_id()
    }

    pub(crate) fn identity(&self) -> BindingIdentity<'_> {
        match &self.binding {
            AdmittedBinding::Codex(binding) => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: binding.harness_version(),
            },
            AdmittedBinding::Claude(binding) => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: binding.harness_version(),
            },
            AdmittedBinding::OpenCode { binding, .. } => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: binding.harness_version(),
            },
        }
    }

    pub(crate) fn has_model_discovery(&self) -> Result<bool, RegistryAdmissionUnavailable> {
        match (&self.binding, self.record.model_provider) {
            (AdmittedBinding::Codex(binding), super::ModelProvider::CodexAccount) => binding
                .model_discovery_available()
                .map_err(RegistryAdmissionUnavailable::Codex),
            (AdmittedBinding::Codex(binding), super::ModelProvider::NoAccountModels)
                if binding.is_desktop_fixture() =>
            {
                Ok(false)
            }
            (AdmittedBinding::Claude(_), super::ModelProvider::ClaudeAdapter) => Ok(false),
            (AdmittedBinding::OpenCode { .. }, super::ModelProvider::NoAccountModels) => Ok(false),
            _ => Err(RegistryAdmissionUnavailable::Drift),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn codex_binding(
        &self,
    ) -> Result<&Arc<AdmittedCodexBinding>, RegistryAdmissionUnavailable> {
        super::codex::binding(&self.binding).ok_or(RegistryAdmissionUnavailable::Drift)
    }

    pub(crate) fn project_root(
        &self,
        database: &crate::persistence::Database,
        context: &crate::state::ActiveProjectContext<'_>,
    ) -> Result<BindingProjectRoot, ()> {
        match self.record.root_policy {
            RootPolicy::Codex => database
                .codex_project_root_authority(context)
                .map(BindingProjectRoot::Codex)
                .map_err(|_| ()),
            RootPolicy::Claude => {
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
            RootPolicy::OpenCode => {
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
                    .map(BindingProjectRoot::OpenCode)
                    .map_err(|_| ())
            }
        }
    }

    pub(crate) fn into_factory(
        self,
        root: BindingProjectRoot,
        mcp: crate::acp::mcp::McpAdmission,
    ) -> Box<dyn RuntimeFactory> {
        (self.record.launch_factory)(self.binding, root, mcp)
    }

    pub(crate) fn supported_capabilities(
        &self,
    ) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
        (self.record.supported_capabilities)(&self.binding)
    }

    pub(crate) fn dispatch_turn(
        &self,
        service: &crate::acp::service::AcpService,
        intent: crate::acp::runtime::HostPromptIntent,
    ) -> Result<
        crate::acp::runtime::HostOutcome,
        crate::acp::service::PromptSendError<crate::acp::service::CodexSessionError>,
    > {
        (self.record.turn_dispatch)(service, intent)
    }
}
