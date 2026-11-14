use std::sync::Arc;

#[cfg(windows)]
mod browser_mcp;
mod models;
mod undo;

use crate::acp::bindings::{
    claude::{AdmittedClaudeBinding, ClaudeBindingRegistration},
    codex::{AdmittedCodexBinding, CodexBindingRegistration},
    runtime::RuntimeFactory,
    standard_acp::{
        AdmittedStandardAcpBinding, CUSTOM_ACP_POLICY_ID, CustomAcpRegistration,
        OpenCodeBindingRegistration, StandardAcpBindingSource,
    },
};
use sage_acp_protocol::intent::CapabilitySnapshotV1;
use sage_domain::harness::{HarnessCharacterisation, ModelOption};

use super::{AuthProvider, ModelProvider, RegistryAdmissionUnavailable, WireIdentity};

pub(crate) use super::record_root::BindingProjectRoot;
pub(super) use super::record_root::RootPolicy;

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
    crate::acp::prompt_send::PromptSendError<crate::acp::service::CodexSessionError>,
>;
type CancelDispatch =
    fn(
        &crate::acp::service::AcpService,
        crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError>;
/// The config option through which a session that starts on the listed
/// `model` takes thinking level `effort`, when the binding offers it then.
type LaunchEffort = fn(&ModelOption, &str) -> Option<&'static str>;
type CloseDispatch =
    fn(
        &crate::acp::service::AcpService,
        crate::acp::store::CloseSessionCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, crate::acp::service::CodexSessionError>;

pub(super) enum Admission {
    Codex(CodexBindingRegistration),
    Claude(Arc<ClaudeBindingRegistration>),
    ClaudeSdk(Arc<crate::acp::bindings::claude::sdk::ClaudeSdkBindingRegistration>),
    OpenCode(Arc<OpenCodeBindingRegistration>),
    CustomAcp(Arc<CustomAcpRegistration>),
    PiNative(Arc<crate::acp::bindings::pi::native_admission::PiBindingRegistration>),
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    Pi(Arc<crate::acp::bindings::pi::fixture_admission::PiFixtureBinding>),
}

pub(crate) struct BoundBuild {
    pub(crate) reported_version: String,
    pub(crate) characterisation: HarnessCharacterisation,
    pub(crate) account_model_discovery: bool,
}

pub(super) enum AdmittedBinding {
    Codex(Arc<AdmittedCodexBinding>),
    Claude(Arc<AdmittedClaudeBinding>),
    ClaudeSdk(Arc<crate::acp::bindings::claude::sdk::AdmittedClaudeSdkBinding>),
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    Pi(Arc<crate::acp::bindings::pi::fixture_admission::PiFixtureBinding>),
    PiNative(Arc<crate::acp::bindings::pi::native_admission::AdmittedPiBinding>),
    StandardAcp {
        source: Arc<dyn StandardAcpBindingSource>,
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
    /// How a session takes a thinking level drafted before it started;
    /// `None` when the binding takes none at start.
    pub(super) launch_effort: Option<LaunchEffort>,
    pub(crate) model_provider: ModelProvider,
    pub(crate) auth_provider: AuthProvider,
}

impl BindingRecord {
    /// `None` when the binding lists no models outside a session: Codex lists
    /// them through account discovery in a project root, and Pi only from
    /// inside a running session.
    pub(crate) fn launch_models(&self, probe: bool) -> Option<Vec<ModelOption>> {
        match &self.admission {
            Admission::ClaudeSdk(registration) => {
                super::claude_sdk::launch_models(registration, probe)
            }
            _ => None,
        }
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
    pub(crate) fn harness_id(&self) -> &'static str {
        self.harness_id
    }
    pub(crate) fn binding_id(&self) -> &'static str {
        self.binding_id
    }

    /// A user-defined agent's record answers to the id of the agent its
    /// registration holds now, never to its internal profile id.
    pub(super) fn serves_harness(&self, id: &str) -> bool {
        match &self.admission {
            Admission::CustomAcp(registration) => registration.serves(id),
            _ => self.harness_id == id,
        }
    }

    pub(super) fn serves_binding(&self, id: &str) -> bool {
        match &self.admission {
            Admission::CustomAcp(registration) => registration.serves(id),
            _ => self.binding_id == id,
        }
    }

    pub(crate) fn access_policy_binding_id(&self) -> &'static str {
        match &self.admission {
            Admission::Codex(_) => crate::acp::bindings::codex::CODEX_BINDING_ID,
            Admission::Claude(_) => crate::acp::bindings::claude::CLAUDE_BINDING_ID,
            Admission::ClaudeSdk(_) => crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID,
            Admission::OpenCode(_) => crate::acp::bindings::standard_acp::OPENCODE_BINDING_ID,
            Admission::CustomAcp(_) => CUSTOM_ACP_POLICY_ID,
            Admission::PiNative(_) => crate::acp::bindings::pi::native_admission::PI_BINDING_ID,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(_) => crate::acp::bindings::pi::fixture_admission::PI_FIXTURE_BINDING_ID,
        }
    }

    pub(crate) fn sends_prompt_blocks(&self) -> bool {
        // The Claude Agent SDK bridge turns an embedded resource into a
        // context text block and a link into an `@path` mention.
        use Admission::{Claude, ClaudeSdk, CustomAcp, OpenCode};
        matches!(
            self.admission,
            Claude(_) | ClaudeSdk(_) | OpenCode(_) | CustomAcp(_)
        )
    }

    pub(crate) fn session_operations_enabled(&self) -> bool {
        self.session_operations_enabled
    }

    pub(crate) fn admit(&self) -> Result<AdmittedBindingRecord<'_>, RegistryAdmissionUnavailable> {
        let binding = match &self.admission {
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(binding) => {
                binding
                    .revalidate()
                    .map_err(|_| RegistryAdmissionUnavailable::Drift)?;
                AdmittedBinding::Pi(binding.clone())
            }
            Admission::Codex(admission) => admission
                .refresh()
                .map(AdmittedBinding::Codex)
                .map_err(RegistryAdmissionUnavailable::Codex)?,
            Admission::Claude(admission) => admission
                .refresh()
                .map(AdmittedBinding::Claude)
                .map_err(RegistryAdmissionUnavailable::Claude)?,
            Admission::ClaudeSdk(admission) => admission
                .refresh()
                .map(AdmittedBinding::ClaudeSdk)
                .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
            Admission::OpenCode(admission) => admission
                .refresh()
                .map(|binding| AdmittedBinding::StandardAcp {
                    source: Arc::clone(admission) as Arc<dyn StandardAcpBindingSource>,
                    binding,
                })
                .map_err(RegistryAdmissionUnavailable::OpenCode)?,
            Admission::CustomAcp(admission) => admission
                .refresh()
                .map(|binding| AdmittedBinding::StandardAcp {
                    source: Arc::clone(admission) as Arc<dyn StandardAcpBindingSource>,
                    binding,
                })
                .map_err(RegistryAdmissionUnavailable::CustomAcp)?,
            Admission::PiNative(admission) => admission
                .refresh()
                .map(AdmittedBinding::PiNative)
                .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
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
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(binding) => binding.revalidate().is_ok(),
            Admission::Codex(admission) => admission.is_available(),
            Admission::Claude(admission) => admission.is_available(),
            Admission::ClaudeSdk(admission) => admission.is_available(),
            Admission::OpenCode(admission) => admission.is_available(),
            Admission::CustomAcp(admission) => admission.admitted().is_some(),
            Admission::PiNative(admission) => admission.is_available(),
        }
    }

    pub(crate) fn bound_build(&self) -> Option<BoundBuild> {
        match &self.admission {
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(binding) => binding.revalidate().ok().map(|()| BoundBuild {
                reported_version: crate::acp::bindings::pi::PI_VERSION.into(),
                characterisation: HarnessCharacterisation::Characterised,
                account_model_discovery: false,
            }),
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
            Admission::ClaudeSdk(admission) => admission.admitted().map(|binding| BoundBuild {
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
            Admission::CustomAcp(admission) => admission.admitted().map(|binding| BoundBuild {
                reported_version: binding.harness_version().to_owned(),
                characterisation: HarnessCharacterisation::Uncharacterised,
                account_model_discovery: false,
            }),
            Admission::PiNative(admission) => admission.admitted().map(|binding| BoundBuild {
                reported_version: binding.harness_version().to_owned(),
                characterisation: binding.characterisation(),
                account_model_discovery: false,
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
    pub(crate) fn binding_id(&self) -> &str {
        match &self.binding {
            AdmittedBinding::StandardAcp { binding, .. } => binding.binding_id(),
            _ => self.record.binding_id(),
        }
    }

    pub(crate) fn identity(&self) -> BindingIdentity<'_> {
        match &self.binding {
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            AdmittedBinding::Pi(binding) => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: crate::acp::bindings::pi::PI_VERSION,
            },
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
            AdmittedBinding::ClaudeSdk(binding) => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: binding.harness_version(),
            },
            AdmittedBinding::PiNative(binding) => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: binding.harness_version(),
            },
            AdmittedBinding::StandardAcp { binding, .. } => BindingIdentity {
                binding_digest: binding.binding_digest(),
                binding_digest_hex: binding.binding_digest_hex(),
                artifact_digest: binding.artifact_digest(),
                harness_version: binding.harness_version(),
            },
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
            RootPolicy::Pi => super::record_root::single_root_authority(database, context)
                .map(BindingProjectRoot::Pi),
            RootPolicy::Codex => database
                .codex_project_root_authority(context)
                .map(BindingProjectRoot::Codex)
                .map_err(|_| ()),
            RootPolicy::Claude => super::record_root::single_root_authority(database, context)
                .map(BindingProjectRoot::Claude),
            RootPolicy::StandardAcp => super::record_root::single_root_authority(database, context)
                .map(BindingProjectRoot::StandardAcp),
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
        crate::acp::prompt_send::PromptSendError<crate::acp::service::CodexSessionError>,
    > {
        (self.record.turn_dispatch)(service, intent)
    }
}
