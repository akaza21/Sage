use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use sha2::{Digest, Sha256};

use crate::harness_discovery::{
    ClaudeLaunchDiscovery, DiscoveredClaudeLaunch, discover_claude_launch,
};

use super::{
    ACP_SDK_VERSION, CLAUDE_BINDING_ID, CandidateManifest, ENTRYPOINT, ManifestError,
    validate_research_candidate,
};

const BINDING_DIGEST_DOMAIN: &[u8] = b"sage:claude-agent-acp:v1:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ClaudeAdmissionUnavailable {
    #[error("the Claude ACP adapter is not installed")]
    AdapterAbsent,
    #[error("the Claude ACP adapter or its ACP SDK version could not be read")]
    AdapterVersionUnreadable,
    #[error("Node.js required by the Claude ACP adapter is not installed")]
    NodeAbsent,
    #[error("the discovered Node.js version could not be read")]
    NodeVersionUnreadable,
    #[error("the Claude ACP adapter bundles an incompatible ACP SDK")]
    AcpSdkMismatch,
    #[error("the discovered Claude adapter does not satisfy its Node engine")]
    NodeEngineMismatch,
    #[error("the discovered Claude launch candidate is invalid")]
    CandidateInvalid,
    #[error("the discovered Claude adapter or Node runtime changed after admission")]
    Drift,
}

/// Native-only authority obtained from discovery. Refresh creates a new
/// candidate, so a package update is adopted rather than treated as a scalar
/// artifact pin; a live candidate still hashes both files before every launch.
#[derive(Debug)]
pub(crate) struct AdmittedClaudeBinding {
    candidate: super::AdmittedCandidate,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
}

impl AdmittedClaudeBinding {
    fn admit(
        discovered: DiscoveredClaudeLaunch,
        state_root: PathBuf,
    ) -> Result<Self, ClaudeAdmissionUnavailable> {
        if discovered.acp_sdk_version != ACP_SDK_VERSION {
            return Err(ClaudeAdmissionUnavailable::AcpSdkMismatch);
        }
        let fixed_argument = discovered.adapter_path.to_string_lossy().into_owned();
        let candidate = validate_research_candidate(
            CandidateManifest {
                adapter_version: discovered.adapter_version,
                acp_sdk_version: discovered.acp_sdk_version,
                entrypoint: ENTRYPOINT.into(),
                node_engine: discovered.node_engine,
                node_version: discovered.node_version,
                artifact_path: discovered.adapter_path,
                artifact_sha256: discovered.adapter_sha256,
                runtime_path: discovered.node_path,
                runtime_sha256: discovered.node_sha256,
                isolated_state_dir: state_root,
                fixed_arguments: vec![fixed_argument],
                environment_keys: vec!["CLAUDE_CONFIG_DIR".into()],
                project_root_count: 1,
            },
            &[],
        )
        .map_err(|error| match error {
            ManifestError::NodeEngineMismatch => ClaudeAdmissionUnavailable::NodeEngineMismatch,
            _ => ClaudeAdmissionUnavailable::CandidateInvalid,
        })?;
        candidate
            .require_launch_admission()
            .map_err(|_| ClaudeAdmissionUnavailable::Drift)?;
        let binding_digest: [u8; 32] = Sha256::new()
            .chain_update(BINDING_DIGEST_DOMAIN)
            .chain_update(candidate.adapter_version.as_bytes())
            .chain_update(candidate.artifact_sha256.as_bytes())
            .chain_update(candidate.runtime_sha256.as_bytes())
            .finalize()
            .into();
        Ok(Self {
            candidate,
            binding_digest_hex: binding_digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            binding_digest,
        })
    }

    pub(crate) fn binding_id(&self) -> &'static str {
        CLAUDE_BINDING_ID
    }
    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.binding_digest
    }
    pub(crate) fn binding_digest_hex(&self) -> &str {
        &self.binding_digest_hex
    }
    pub(crate) fn harness_version(&self) -> &str {
        &self.candidate.adapter_version
    }
    pub(crate) fn candidate(&self) -> &super::AdmittedCandidate {
        &self.candidate
    }

    pub(crate) fn launch_environment(
        &self,
    ) -> Result<Vec<(OsString, OsString)>, ClaudeAdmissionUnavailable> {
        let state = self.candidate.isolated_state_dir.as_os_str();
        if state.is_empty() {
            return Err(ClaudeAdmissionUnavailable::CandidateInvalid);
        }
        Ok(vec![(
            OsString::from("CLAUDE_CONFIG_DIR"),
            state.to_os_string(),
        )])
    }

    pub(crate) fn revalidate(&self) -> Result<(), ClaudeAdmissionUnavailable> {
        self.candidate
            .require_launch_admission()
            .map_err(|_| ClaudeAdmissionUnavailable::Drift)
    }
}

#[derive(Debug)]
enum RegistrationState {
    Unavailable(ClaudeAdmissionUnavailable),
    Admitted(Arc<AdmittedClaudeBinding>),
}

#[derive(Debug)]
pub(crate) struct ClaudeBindingRegistration {
    state_root: PathBuf,
    state: Mutex<RegistrationState>,
}

impl ClaudeBindingRegistration {
    pub(crate) fn startup() -> Self {
        let state_root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Sage")
            .join("claude-agent-acp");
        let discovery = discover_claude_launch();
        Self::from_discovery(discovery, state_root)
    }

    fn from_discovery(discovery: ClaudeLaunchDiscovery, state_root: PathBuf) -> Self {
        let state = match discovery {
            ClaudeLaunchDiscovery::AdapterAbsent => {
                RegistrationState::Unavailable(ClaudeAdmissionUnavailable::AdapterAbsent)
            }
            ClaudeLaunchDiscovery::AdapterVersionUnreadable => {
                RegistrationState::Unavailable(ClaudeAdmissionUnavailable::AdapterVersionUnreadable)
            }
            ClaudeLaunchDiscovery::NodeAbsent => {
                RegistrationState::Unavailable(ClaudeAdmissionUnavailable::NodeAbsent)
            }
            ClaudeLaunchDiscovery::NodeVersionUnreadable => {
                RegistrationState::Unavailable(ClaudeAdmissionUnavailable::NodeVersionUnreadable)
            }
            ClaudeLaunchDiscovery::Present(candidate) => {
                match AdmittedClaudeBinding::admit(*candidate, state_root.clone()) {
                    Ok(binding) => RegistrationState::Admitted(Arc::new(binding)),
                    Err(reason) => RegistrationState::Unavailable(reason),
                }
            }
        };
        Self {
            state_root,
            state: Mutex::new(state),
        }
    }

    pub(crate) fn refresh(&self) -> Result<Arc<AdmittedClaudeBinding>, ClaudeAdmissionUnavailable> {
        let candidate = match &*self.lock_state() {
            RegistrationState::Admitted(binding) => Arc::clone(binding),
            RegistrationState::Unavailable(reason) => return Err(*reason),
        };
        let validation = candidate.revalidate();
        let mut state = self.lock_state();
        match &*state {
            RegistrationState::Admitted(current) if Arc::ptr_eq(current, &candidate) => {
                match validation {
                    Ok(()) => Ok(candidate),
                    Err(reason) => {
                        *state = RegistrationState::Unavailable(reason);
                        Err(reason)
                    }
                }
            }
            RegistrationState::Admitted(_) => Err(ClaudeAdmissionUnavailable::Drift),
            RegistrationState::Unavailable(reason) => Err(*reason),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Admitted(_))
    }
    pub(crate) fn unavailable(&self) -> Option<ClaudeAdmissionUnavailable> {
        match *self.lock_state() {
            RegistrationState::Unavailable(reason) => Some(reason),
            RegistrationState::Admitted(_) => None,
        }
    }
    fn lock_state(&self) -> std::sync::MutexGuard<'_, RegistrationState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                *state = RegistrationState::Unavailable(ClaudeAdmissionUnavailable::Drift);
                state
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn admitted_for_test(discovery: ClaudeLaunchDiscovery, state_root: PathBuf) -> Self {
        Self::from_discovery(discovery, state_root)
    }

    #[cfg(test)]
    pub(crate) fn state_root(&self) -> &std::path::Path {
        &self.state_root
    }
}
