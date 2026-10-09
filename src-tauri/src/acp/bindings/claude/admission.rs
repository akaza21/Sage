use std::{
    ffi::OsString,
    sync::{Arc, Mutex},
};

#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

use sha2::{Digest, Sha256};

use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};

use super::{
    CLAUDE_BINDING_ID, CandidateManifest, ENTRYPOINT, ManifestError, validate_research_candidate,
};

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[path = "fixture_admission.rs"]
mod fixture_admission;

const BINDING_DIGEST_DOMAIN: &[u8] = b"sage:claude-agent-acp:v1:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ClaudeAdmissionUnavailable {
    #[error("Claude launch admission discovery is still in progress")]
    DiscoveryPending,
    #[error("the Claude ACP adapter is not installed")]
    AdapterAbsent,
    #[error("the Claude ACP adapter or its ACP SDK version could not be read")]
    AdapterVersionUnreadable,
    #[error("Node.js required by the Claude ACP adapter is not installed")]
    NodeAbsent,
    #[error("the discovered Node.js version could not be read")]
    NodeVersionUnreadable,
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
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    fixture: Option<crate::launch_profile::FixtureSelection>,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
    launch_environment: Vec<(OsString, OsString)>,
    #[cfg(windows)]
    system_root: OsString,
}

impl AdmittedClaudeBinding {
    fn admit(discovered: DiscoveredClaudeLaunch) -> Result<Self, ClaudeAdmissionUnavailable> {
        #[cfg(windows)]
        let system_root = system_windows_directory()?;
        let fixed_argument = discovered.adapter_path.to_string_lossy().into_owned();
        let candidate = validate_research_candidate(CandidateManifest {
            adapter_version: discovered.adapter_version,
            acp_sdk_version: discovered.acp_sdk_version,
            entrypoint: ENTRYPOINT.into(),
            node_engine: discovered.node_engine,
            node_version: discovered.node_version,
            artifact_path: discovered.adapter_path,
            artifact_sha256: discovered.adapter_sha256,
            runtime_path: discovered.node_path,
            runtime_sha256: discovered.node_sha256,
            fixed_arguments: vec![fixed_argument],
            environment_keys: super::LAUNCH_ENVIRONMENT_KEYS
                .iter()
                .map(|key| (*key).into())
                .collect(),
            project_root_count: 1,
        })
        .map_err(|error| match error {
            ManifestError::NodeEngineMismatch => ClaudeAdmissionUnavailable::NodeEngineMismatch,
            _ => ClaudeAdmissionUnavailable::CandidateInvalid,
        })?;
        candidate
            .require_launch_admission()
            .map_err(|_| ClaudeAdmissionUnavailable::Drift)?;
        let launch_environment = current_launch_environment(
            #[cfg(windows)]
            &system_root,
        )?;
        let binding_digest: [u8; 32] = Sha256::new()
            .chain_update(BINDING_DIGEST_DOMAIN)
            .chain_update(candidate.adapter_version.as_bytes())
            .chain_update(candidate.artifact_sha256.as_bytes())
            .chain_update(candidate.runtime_sha256.as_bytes())
            .finalize()
            .into();
        Ok(Self {
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            fixture: None,
            candidate,
            binding_digest_hex: binding_digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            binding_digest,
            #[cfg(windows)]
            system_root,
            launch_environment,
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
    pub(crate) fn artifact_digest(&self) -> &str {
        &self.candidate.artifact_sha256
    }
    pub(crate) fn harness_version(&self) -> &str {
        &self.candidate.adapter_version
    }
    pub(crate) fn characterisation(
        &self,
    ) -> crate::acp::bindings::characterisation::Characterisation {
        self.candidate.characterisation
    }
    pub(crate) fn candidate(&self) -> &super::AdmittedCandidate {
        &self.candidate
    }

    pub(crate) fn launch_environment(
        &self,
    ) -> Result<Vec<(OsString, OsString)>, ClaudeAdmissionUnavailable> {
        self.revalidate()?;
        Ok(self.launch_environment.clone())
    }

    pub(crate) fn revalidate(&self) -> Result<(), ClaudeAdmissionUnavailable> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(selection) = &self.fixture {
            return selection
                .revalidate()
                .map_err(|_| ClaudeAdmissionUnavailable::Drift);
        }
        self.candidate
            .require_launch_admission()
            .map_err(|_| ClaudeAdmissionUnavailable::Drift)?;
        #[cfg(windows)]
        self.revalidate_system_root()?;
        Ok(())
    }

    #[cfg(windows)]
    fn revalidate_system_root(&self) -> Result<(), ClaudeAdmissionUnavailable> {
        let current = system_windows_directory()?;
        if !current
            .to_string_lossy()
            .eq_ignore_ascii_case(&self.system_root.to_string_lossy())
        {
            return Err(ClaudeAdmissionUnavailable::Drift);
        }
        Ok(())
    }
}

#[cfg(windows)]
fn current_launch_environment(
    system_root: &OsString,
) -> Result<Vec<(OsString, OsString)>, ClaudeAdmissionUnavailable> {
    let user_profile = required_process_value("USERPROFILE")?;
    let config_directory = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(&user_profile)
                .join(".claude")
                .into_os_string()
        });
    let values = [
        config_directory,
        user_profile,
        required_process_value("APPDATA")?,
        required_process_value("LOCALAPPDATA")?,
        required_process_value("PATH")?,
        required_process_value("PATHEXT")?,
        required_process_value("ComSpec")?,
        system_root.clone(),
        required_process_value("TEMP")?,
        required_process_value("TMP")?,
    ];
    Ok(super::LAUNCH_ENVIRONMENT_KEYS
        .iter()
        .zip(values)
        .map(|(key, value)| (OsString::from(key), value))
        .collect())
}

#[cfg(not(windows))]
fn current_launch_environment() -> Result<Vec<(OsString, OsString)>, ClaudeAdmissionUnavailable> {
    unix_launch_environment(|key| std::env::var_os(key))
}

#[cfg(any(not(windows), test))]
pub(crate) fn unix_launch_environment(
    read: impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<(OsString, OsString)>, ClaudeAdmissionUnavailable> {
    let present = |key: &str| read(key).filter(|value| !value.is_empty());
    let home = present("HOME").ok_or(ClaudeAdmissionUnavailable::CandidateInvalid)?;
    let path = present("PATH").ok_or(ClaudeAdmissionUnavailable::CandidateInvalid)?;
    let config_directory = present("CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|| std::path::Path::new(&home).join(".claude").into_os_string());
    let resolved = [
        ("CLAUDE_CONFIG_DIR", Some(config_directory)),
        ("HOME", Some(home)),
        ("PATH", Some(path)),
        ("TMPDIR", present("TMPDIR")),
    ];
    Ok(resolved
        .into_iter()
        .filter(|(key, _)| super::manifest::UNIX_LAUNCH_ENVIRONMENT_KEYS.contains(key))
        .filter_map(|(key, value)| Some((OsString::from(key), value?)))
        .collect())
}

#[cfg(windows)]
fn required_process_value(key: &str) -> Result<OsString, ClaudeAdmissionUnavailable> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .ok_or(ClaudeAdmissionUnavailable::CandidateInvalid)
}

#[cfg(windows)]
fn system_windows_directory() -> Result<OsString, ClaudeAdmissionUnavailable> {
    const WINDOWS_DIRECTORY_WIDE_MAX: usize = 32 * 1024;

    let mut buffer = vec![0_u16; WINDOWS_DIRECTORY_WIDE_MAX];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW(
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len())
                .map_err(|_| ClaudeAdmissionUnavailable::CandidateInvalid)?,
        )
    };
    let length =
        usize::try_from(length).map_err(|_| ClaudeAdmissionUnavailable::CandidateInvalid)?;
    if length == 0 || length >= buffer.len() {
        return Err(ClaudeAdmissionUnavailable::CandidateInvalid);
    }
    Ok(OsString::from_wide(&buffer[..length]))
}

#[cfg(all(test, windows))]
pub(crate) fn system_windows_directory_for_test() -> Result<OsString, ClaudeAdmissionUnavailable> {
    system_windows_directory()
}

#[derive(Debug)]
enum RegistrationState {
    Pending,
    Unavailable(ClaudeAdmissionUnavailable),
    Admitted(Arc<AdmittedClaudeBinding>),
}

#[derive(Debug)]
pub(crate) struct ClaudeBindingRegistration {
    state: Mutex<RegistrationState>,
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    fixture_only: bool,
}

impl ClaudeBindingRegistration {
    pub(crate) fn startup() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Pending),
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            fixture_only: false,
        }
    }

    fn from_discovery(discovery: ClaudeLaunchDiscovery) -> Self {
        Self {
            state: Mutex::new(Self::state_from_discovery(discovery)),
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            fixture_only: false,
        }
    }

    fn state_from_discovery(discovery: ClaudeLaunchDiscovery) -> RegistrationState {
        match discovery {
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
                match AdmittedClaudeBinding::admit(*candidate) {
                    Ok(binding) => RegistrationState::Admitted(Arc::new(binding)),
                    Err(reason) => RegistrationState::Unavailable(reason),
                }
            }
        }
    }

    pub(crate) fn apply_discovery(&self, discovery: ClaudeLaunchDiscovery) {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if self.fixture_only {
            return;
        }
        let next = Self::state_from_discovery(discovery);
        *self.lock_state() = next;
    }

    pub(crate) fn refresh(&self) -> Result<Arc<AdmittedClaudeBinding>, ClaudeAdmissionUnavailable> {
        let candidate = match &*self.lock_state() {
            RegistrationState::Admitted(binding) => Arc::clone(binding),
            RegistrationState::Pending => return Err(ClaudeAdmissionUnavailable::DiscoveryPending),
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
            RegistrationState::Pending => Err(ClaudeAdmissionUnavailable::DiscoveryPending),
            RegistrationState::Unavailable(reason) => Err(*reason),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Admitted(_))
    }

    pub(crate) fn admitted(&self) -> Option<Arc<AdmittedClaudeBinding>> {
        match &*self.lock_state() {
            RegistrationState::Admitted(binding) => Some(Arc::clone(binding)),
            RegistrationState::Pending => None,
            RegistrationState::Unavailable(_) => None,
        }
    }
    pub(crate) fn unavailable(&self) -> Option<ClaudeAdmissionUnavailable> {
        match *self.lock_state() {
            RegistrationState::Unavailable(reason) => Some(reason),
            RegistrationState::Pending => Some(ClaudeAdmissionUnavailable::DiscoveryPending),
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
    pub(crate) fn admitted_for_test(discovery: ClaudeLaunchDiscovery) -> Self {
        Self::from_discovery(discovery)
    }

    #[cfg(test)]
    pub(crate) fn absent_for_test() -> Self {
        Self::from_discovery(ClaudeLaunchDiscovery::AdapterAbsent)
    }
}
