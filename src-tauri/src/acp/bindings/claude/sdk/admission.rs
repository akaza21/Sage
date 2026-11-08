//! Admission of the installed Claude Code CLI for the SDK protocol.
//!
//! Discovery reports the native `claude` executable and its version. Sage admits a
//! CLI from the release line whose SDK protocol this binding covers, pins the
//! exact file by SHA-256, and revalidates it before every launch.

use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use sha2::{Digest, Sha256};

use crate::{
    acp::{
        bindings::adapter::AdapterRuntimeError,
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
            RequiredContainment, TreeLimits,
        },
    },
    domain::{HarnessCharacterisation, HarnessUnavailableReason},
    harness_discovery::InstalledHarness,
};

pub(crate) const CLAUDE_SDK_BINDING_ID: &str = "claude-code-sdk";
/// The Claude Code release whose SDK protocol this binding was characterised on.
pub(crate) const CLAUDE_CODE_VERSION: &str = "2.1.319";
/// The oldest release on the same line with every control request Sage sends.
const SUPPORTED_MINIMUM: (u64, u64, u64) = (2, 1, 300);
const BINDING_DIGEST_DOMAIN: &[u8] = b"sage:claude-code-sdk:v1:";

/// Fixed SDK-mode arguments; the session arguments follow them.
const SDK_ARGS: [&str; 10] = [
    "--output-format",
    "stream-json",
    "--input-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--permission-prompt-tool",
    "stdio",
    "--setting-sources",
    "user,project,local",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ClaudeSdkAdmissionUnavailable {
    #[error("Claude Code discovery is still in progress")]
    DiscoveryPending,
    #[error("Claude Code is not installed")]
    Absent,
    #[error("the installed Claude Code version could not be read or is not supported")]
    VersionUnsupported,
    #[error("the Claude Code executable could not be read")]
    Unreadable,
    #[error("the Claude Code executable changed after admission")]
    Drift,
}

impl From<ClaudeSdkAdmissionUnavailable> for HarnessUnavailableReason {
    fn from(reason: ClaudeSdkAdmissionUnavailable) -> Self {
        use ClaudeSdkAdmissionUnavailable as Sdk;
        match reason {
            Sdk::DiscoveryPending => Self::AdmissionIncomplete,
            Sdk::Absent | Sdk::VersionUnsupported | Sdk::Unreadable => Self::CandidateInvalid,
            Sdk::Drift => Self::DriftDetected,
        }
    }
}

#[derive(Debug)]
pub(crate) struct AdmittedClaudeSdkBinding {
    executable: PathBuf,
    version: String,
    sha256: [u8; 32],
    sha256_hex: String,
    byte_len: u64,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
}

/// The session a launch opens: a fresh one with a Sage-chosen id, or an
/// existing one to resume.
pub(crate) struct SessionLaunch {
    pub(crate) session_id: String,
    pub(crate) resume: bool,
    pub(crate) model: Option<String>,
}

impl SessionLaunch {
    fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = SDK_ARGS.map(OsString::from).into();
        args.push(
            if self.resume {
                "--resume"
            } else {
                "--session-id"
            }
            .into(),
        );
        args.push(self.session_id.clone().into());
        if let Some(model) = &self.model {
            args.extend(["--model".into(), model.into()]);
        }
        args
    }
}

fn version_admitted(version: &str) -> bool {
    let parse = |value: &str| -> Option<(u64, u64, u64)> {
        let mut parts = value.split('.').map(str::parse::<u64>);
        let version = (
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        );
        parts.next().is_none().then_some(version)
    };
    parse(version).is_some_and(|found| found >= SUPPORTED_MINIMUM && found.0 == SUPPORTED_MINIMUM.0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl AdmittedClaudeSdkBinding {
    fn admit(installed: &InstalledHarness) -> Result<Self, ClaudeSdkAdmissionUnavailable> {
        let version = installed
            .version
            .clone()
            .filter(|version| version_admitted(version))
            .ok_or(ClaudeSdkAdmissionUnavailable::VersionUnsupported)?;
        let executable = std::fs::canonicalize(&installed.executable)
            .map_err(|_| ClaudeSdkAdmissionUnavailable::Unreadable)?;
        let sha256_hex = crate::harness_discovery::cached_sha256_file(&executable)
            .ok_or(ClaudeSdkAdmissionUnavailable::Unreadable)?;
        let mut sha256 = [0_u8; 32];
        for (byte, pair) in sha256.iter_mut().zip(sha256_hex.as_bytes().chunks(2)) {
            *byte = std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(ClaudeSdkAdmissionUnavailable::Unreadable)?;
        }
        let byte_len = std::fs::metadata(&executable)
            .map_err(|_| ClaudeSdkAdmissionUnavailable::Unreadable)?
            .len();
        let binding_digest: [u8; 32] = Sha256::new()
            .chain_update(BINDING_DIGEST_DOMAIN)
            .chain_update(version.as_bytes())
            .chain_update(sha256)
            .finalize()
            .into();
        Ok(Self {
            executable,
            version,
            sha256,
            sha256_hex,
            byte_len,
            binding_digest_hex: hex(&binding_digest),
            binding_digest,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            executable: PathBuf::from("/fixture/claude"),
            version: CLAUDE_CODE_VERSION.into(),
            sha256: [1; 32],
            sha256_hex: hex(&[1; 32]),
            byte_len: 1,
            binding_digest: [7; 32],
            binding_digest_hex: hex(&[7; 32]),
        }
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.binding_digest
    }
    pub(crate) fn binding_digest_hex(&self) -> &str {
        &self.binding_digest_hex
    }
    pub(crate) fn artifact_digest(&self) -> &str {
        &self.sha256_hex
    }
    pub(crate) fn harness_version(&self) -> &str {
        &self.version
    }
    pub(crate) fn characterisation(&self) -> HarnessCharacterisation {
        if self.version == CLAUDE_CODE_VERSION {
            HarnessCharacterisation::Characterised
        } else {
            HarnessCharacterisation::Uncharacterised
        }
    }

    pub(crate) fn revalidate(&self) -> Result<(), ClaudeSdkAdmissionUnavailable> {
        (crate::harness_discovery::cached_sha256_file(&self.executable).as_deref()
            == Some(self.sha256_hex.as_str()))
        .then_some(())
        .ok_or(ClaudeSdkAdmissionUnavailable::Drift)
    }

    pub(crate) fn launch_spec(
        &self,
        epoch: u64,
        root: crate::project_registry::ProjectRootAuthority,
        session: &SessionLaunch,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        fn unavailable<E>(_: E) -> AdapterRuntimeError {
            AdapterRuntimeError::LaunchUnavailable
        }
        self.revalidate().map_err(unavailable)?;
        root.revalidate().map_err(unavailable)?;
        let mut identity = [0_u8; 16];
        identity.copy_from_slice(&self.binding_digest[..16]);
        let identity = uuid::Uuid::from_bytes(identity);
        let artifact = ArtifactExpectation::new(
            identity,
            self.executable.clone(),
            self.sha256,
            self.byte_len,
        );
        Ok(LaunchSpec {
            binding_id: identity,
            runtime_epoch: epoch,
            artifact: artifact.map_err(unavailable)?,
            fixed_args: BoundedOsArgs::new(session.args()).map_err(unavailable)?,
            environment: BoundedEnvironment::new(launch_environment()).map_err(unavailable)?,
            project_root: root,
            required_guarantee: if cfg!(windows) {
                RequiredContainment::EnforcedTree
            } else {
                RequiredContainment::TrustedCooperativeTree
            },
            limits: TreeLimits::ade_0006a(),
            deadlines: ProcessDeadlines::ade_0006a(),
            absolute_cleanup_deadline: None,
            #[cfg(all(test, windows))]
            test_after_artifact_verified: None,
        })
    }
}

/// The variables Claude Code needs to find its profile, credentials and
/// tools, as the ACP Claude launch allows.
pub(crate) fn launch_environment() -> Vec<(OsString, OsString)> {
    crate::acp::bindings::claude::LAUNCH_ENVIRONMENT_KEYS
        .iter()
        .filter_map(|key| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(|value| (OsString::from(key), value))
        })
        .collect()
}

#[derive(Debug)]
enum RegistrationState {
    Pending,
    Unavailable(ClaudeSdkAdmissionUnavailable),
    Admitted(Arc<AdmittedClaudeSdkBinding>),
}

#[derive(Debug)]
pub(crate) struct ClaudeSdkBindingRegistration {
    state: Mutex<RegistrationState>,
}

impl ClaudeSdkBindingRegistration {
    pub(crate) fn startup() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Pending),
        }
    }

    pub(crate) fn apply_discovery(&self, installed: Option<&InstalledHarness>) {
        *self.lock_state() = match installed.map(AdmittedClaudeSdkBinding::admit) {
            None => RegistrationState::Unavailable(ClaudeSdkAdmissionUnavailable::Absent),
            Some(Ok(binding)) => RegistrationState::Admitted(Arc::new(binding)),
            Some(Err(reason)) => RegistrationState::Unavailable(reason),
        };
    }

    pub(crate) fn refresh(
        &self,
    ) -> Result<Arc<AdmittedClaudeSdkBinding>, ClaudeSdkAdmissionUnavailable> {
        let mut state = self.lock_state();
        let binding = match &*state {
            RegistrationState::Admitted(binding) => Arc::clone(binding),
            RegistrationState::Pending => {
                return Err(ClaudeSdkAdmissionUnavailable::DiscoveryPending);
            }
            RegistrationState::Unavailable(reason) => return Err(*reason),
        };
        binding.revalidate().inspect_err(|reason| {
            *state = RegistrationState::Unavailable(*reason);
        })?;
        Ok(binding)
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Admitted(_))
    }

    pub(crate) fn admitted(&self) -> Option<Arc<AdmittedClaudeSdkBinding>> {
        match &*self.lock_state() {
            RegistrationState::Admitted(binding) => Some(Arc::clone(binding)),
            _ => None,
        }
    }

    pub(crate) fn unavailable(&self) -> Option<ClaudeSdkAdmissionUnavailable> {
        match *self.lock_state() {
            RegistrationState::Pending => Some(ClaudeSdkAdmissionUnavailable::DiscoveryPending),
            RegistrationState::Unavailable(reason) => Some(reason),
            RegistrationState::Admitted(_) => None,
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, RegistrationState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_the_supported_release_line_only() {
        for (version, admitted) in [
            ("2.1.319", true),
            ("2.1.300", true),
            ("2.2.0", true),
            ("2.1.299", false),
            ("3.0.0", false),
            ("2.1", false),
            ("2.1.319-beta", false),
        ] {
            assert_eq!(version_admitted(version), admitted, "{version}");
        }
    }

    #[test]
    fn new_sessions_take_sages_id_and_resumes_name_theirs() {
        let mut launch = SessionLaunch {
            session_id: "s-1".into(),
            resume: false,
            model: Some("opus".into()),
        };
        let args = launch.args();
        let tail: Vec<_> = args[SDK_ARGS.len()..]
            .iter()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert_eq!(tail, ["--session-id", "s-1", "--model", "opus"]);
        launch.resume = true;
        launch.model = None;
        let args = launch.args();
        assert_eq!(
            args[SDK_ARGS.len()..],
            [OsString::from("--resume"), "s-1".into()]
        );
    }

    #[test]
    fn a_missing_or_unsupported_cli_is_reported_not_admitted() {
        let registration = ClaudeSdkBindingRegistration::startup();
        assert_eq!(
            registration.unavailable(),
            Some(ClaudeSdkAdmissionUnavailable::DiscoveryPending)
        );
        registration.apply_discovery(None);
        assert_eq!(
            registration.unavailable(),
            Some(ClaudeSdkAdmissionUnavailable::Absent)
        );
        registration.apply_discovery(Some(&InstalledHarness {
            id: "claude-code",
            executable: PathBuf::from("/nonexistent/claude"),
            version: Some("2.0.1".into()),
        }));
        assert_eq!(
            registration.unavailable(),
            Some(ClaudeSdkAdmissionUnavailable::VersionUnsupported)
        );
    }
}
