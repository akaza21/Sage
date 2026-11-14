//! Sage admits a CLI from the release line whose SDK protocol this binding covers, pins the
//! exact file by SHA-256, and revalidates it before every launch.

use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use sha2::{Digest, Sha256};

use crate::{
    acp::{
        bindings::{adapter::AdapterRuntimeError, release_version::ReleaseVersion},
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
            RequiredContainment, TreeLimits,
        },
    },
    domain::{HarnessCharacterisation, HarnessUnavailableReason},
    harness_discovery::InstalledHarness,
};

use super::launch::SessionLaunch;

pub(crate) const CLAUDE_SDK_BINDING_ID: &str = "claude-code-sdk";
#[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
pub(crate) const CLAUDE_CODE_VERSION: &str = "2.1.319";
/// The oldest release with every control request Sage sends; an earlier one
/// is refused because those requests would fail.
const SUPPORTED_MINIMUM: &str = "2.1.300";
const BINDING_DIGEST_DOMAIN: &[u8] = b"sage:claude-code-sdk:v1:";

/// The mode is forced to `default` so a settings file cannot start Claude Code in a mode the
/// picker does not show (bypass would skip every approval).
pub(super) const SDK_ARGS: [&str; 12] = [
    "--output-format",
    "stream-json",
    "--input-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--permission-prompt-tool",
    "stdio",
    "--permission-mode",
    "default",
    "--setting-sources",
    "user,project,local",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ClaudeSdkAdmissionUnavailable {
    #[error("Claude Code discovery is still in progress")]
    DiscoveryPending,
    #[error("Claude Code is not installed")]
    Absent,
    #[error("the installed Claude Code version could not be read")]
    VersionUnreadable,
    #[error("the installed Claude Code version is older than Sage supports")]
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
            Sdk::Absent | Sdk::Unreadable | Sdk::VersionUnreadable => Self::CandidateInvalid,
            Sdk::VersionUnsupported => Self::VersionTooOld,
            Sdk::Drift => Self::DriftDetected,
        }
    }
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
type Fixture = Option<crate::launch_profile::FixtureSelection>;
#[cfg(not(all(windows, feature = "acp-fixture", debug_assertions)))]
type Fixture = ();

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[path = "fixture_admission.rs"]
mod fixture;

#[derive(Debug)]
pub(crate) struct AdmittedClaudeSdkBinding {
    /// The path discovery found; an update can repoint it at a new file.
    launcher: PathBuf,
    executable: PathBuf,
    version: String,
    sha256: [u8; 32],
    sha256_hex: String,
    byte_len: u64,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
    #[cfg_attr(
        not(all(windows, feature = "acp-fixture", debug_assertions)),
        expect(dead_code, reason = "only desktop fixture builds launch a replay case")
    )]
    fixture: Fixture,
}

fn version_characterisation(version: &str) -> HarnessCharacterisation {
    let (Some(found), Some(minimum)) = (
        ReleaseVersion::parse(version),
        ReleaseVersion::parse(SUPPORTED_MINIMUM),
    ) else {
        return HarnessCharacterisation::Uncharacterised;
    };
    if found < minimum {
        HarnessCharacterisation::Uncharacterised
    } else if found.same_minor_line(&minimum) && !found.is_prerelease() {
        HarnessCharacterisation::Characterised
    } else {
        HarnessCharacterisation::NewerThanTested
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl AdmittedClaudeSdkBinding {
    fn admit(installed: &InstalledHarness) -> Result<Self, ClaudeSdkAdmissionUnavailable> {
        let version = installed
            .version
            .clone()
            .filter(|version| ReleaseVersion::parse(version).is_some())
            .ok_or(ClaudeSdkAdmissionUnavailable::VersionUnreadable)?;
        if !version_characterisation(&version).runs_tested_features() {
            return Err(ClaudeSdkAdmissionUnavailable::VersionUnsupported);
        }
        let executable = crate::harness_discovery::claude_sdk_executable(&installed.executable)
            .ok_or(ClaudeSdkAdmissionUnavailable::Unreadable)?;
        // A native launcher can be repointed after the version probe.
        // Its target names the version, so refuse a mismatched observation.
        #[cfg(unix)]
        if executable.file_name() != installed.executable.file_name()
            && executable.file_name().and_then(std::ffi::OsStr::to_str) != Some(version.as_str())
        {
            return Err(ClaudeSdkAdmissionUnavailable::Drift);
        }
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
            launcher: installed.executable.clone(),
            executable,
            version,
            sha256,
            sha256_hex,
            byte_len,
            binding_digest_hex: hex(&binding_digest),
            binding_digest,
            fixture: Fixture::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            launcher: PathBuf::from("/fixture/claude"),
            executable: PathBuf::from("/fixture/claude"),
            version: CLAUDE_CODE_VERSION.into(),
            sha256: [1; 32],
            sha256_hex: hex(&[1; 32]),
            byte_len: 1,
            binding_digest: [7; 32],
            binding_digest_hex: hex(&[7; 32]),
            fixture: Fixture::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_digest(digest: [u8; 32]) -> Self {
        Self {
            binding_digest: digest,
            ..Self::for_test()
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
    pub(super) fn probe_executable(&self) -> Option<&std::path::Path> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if self.fixture.is_some() {
            return None;
        }
        Some(&self.executable)
    }
    pub(crate) fn harness_version(&self) -> &str {
        &self.version
    }
    pub(crate) fn characterisation(&self) -> HarnessCharacterisation {
        version_characterisation(&self.version)
    }

    pub(crate) fn revalidate(&self) -> Result<(), ClaudeSdkAdmissionUnavailable> {
        let current = self.launcher == self.executable
            || std::fs::canonicalize(&self.launcher).is_ok_and(|path| path == self.executable);
        (current
            && crate::harness_discovery::cached_sha256_file(&self.executable).as_deref()
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
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(selection) = &self.fixture {
            let session = session.session_args(Some(&selection.durable_dir))?;
            return self.fixture_launch(selection, epoch, root, session);
        }
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
        // Claude Code checkpoints the files each turn changes, so a turn
        // can be undone.
        let mut environment = launch_environment();
        let config = super::fork::config_directory(&environment);
        environment.push((
            "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING".into(),
            "true".into(),
        ));
        Ok(LaunchSpec {
            binding_id: identity,
            runtime_epoch: epoch,
            artifact: artifact.map_err(unavailable)?,
            fixed_args: BoundedOsArgs::new(session.args(config.as_deref())?)
                .map_err(unavailable)?,
            environment: BoundedEnvironment::new(environment).map_err(unavailable)?,
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

impl AdmittedClaudeSdkBinding {
    pub(crate) fn config_directory(&self) -> Option<PathBuf> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(selection) = &self.fixture {
            return Some(selection.durable_dir.clone());
        }
        super::fork::config_directory(&launch_environment())
    }
}

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
    fixture_only: bool,
}

impl ClaudeSdkBindingRegistration {
    pub(crate) fn startup() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Pending),
            fixture_only: false,
        }
    }

    pub(crate) fn apply_discovery(&self, installed: Option<&InstalledHarness>) {
        if self.fixture_only {
            return;
        }
        *self.lock_state() = match installed.map(AdmittedClaudeSdkBinding::admit) {
            None => RegistrationState::Unavailable(ClaudeSdkAdmissionUnavailable::Absent),
            Some(Ok(binding)) => RegistrationState::Admitted(Arc::new(binding)),
            Some(Err(reason)) => RegistrationState::Unavailable(reason),
        };
    }

    /// Claude Code updates itself, in place or by repointing its launcher, so a changed file still
    /// on the supported line is admitted again.
    pub(crate) fn refresh(
        &self,
    ) -> Result<Arc<AdmittedClaudeSdkBinding>, ClaudeSdkAdmissionUnavailable> {
        let binding = match &*self.lock_state() {
            RegistrationState::Admitted(binding) => Arc::clone(binding),
            RegistrationState::Pending => {
                return Err(ClaudeSdkAdmissionUnavailable::DiscoveryPending);
            }
            RegistrationState::Unavailable(reason) => return Err(*reason),
        };
        let Err(drift) = binding.revalidate() else {
            return Ok(binding);
        };
        if self.fixture_only {
            return Err(drift);
        }
        // Probed through the launcher outside the lock. A failed probe is not
        // kept, so the next launch probes again, and a state discovery or
        // another refresh replaced meanwhile wins.
        let version =
            crate::harness_discovery::revalidate_harness_version("claude-code", &binding.launcher);
        let readmitted = AdmittedClaudeSdkBinding::admit(&InstalledHarness {
            id: "claude-code",
            executable: binding.launcher.clone(),
            version,
        })
        .map(Arc::new)?;
        let mut state = self.lock_state();
        match &*state {
            RegistrationState::Admitted(current) if Arc::ptr_eq(current, &binding) => {
                *state = RegistrationState::Admitted(Arc::clone(&readmitted));
                Ok(readmitted)
            }
            RegistrationState::Admitted(current) => Ok(Arc::clone(current)),
            RegistrationState::Pending => Err(ClaudeSdkAdmissionUnavailable::DiscoveryPending),
            RegistrationState::Unavailable(reason) => Err(*reason),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Admitted(_))
    }

    pub(crate) fn discovery_pending(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Pending)
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
    fn admits_every_release_from_the_supported_minimum() {
        for (version, admitted) in [
            ("2.1.319", true),
            ("2.1.300", true),
            ("2.1.400", true),
            ("2.2.0", true),
            ("3.0.0", true),
            ("2.1.320-beta.1", true),
            ("2.1.299", false),
            ("2.1.300-rc.1", false),
            ("2.1", false),
        ] {
            assert_eq!(
                version_characterisation(version).runs_tested_features(),
                admitted,
                "{version}"
            );
        }
    }

    #[test]
    fn the_2_1_line_is_tested_and_later_lines_are_newer_than_tested() {
        use HarnessCharacterisation::{Characterised, NewerThanTested, Uncharacterised};
        for (version, expected) in [
            ("2.1.300", Characterised),
            ("2.1.319", Characterised),
            ("2.1.1000", Characterised),
            ("2.1.320-beta.1", NewerThanTested),
            ("2.2.0", NewerThanTested),
            ("3.0.0", NewerThanTested),
            ("2.1.299", Uncharacterised),
        ] {
            assert_eq!(version_characterisation(version), expected, "{version}");
        }
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
        assert_eq!(
            HarnessUnavailableReason::from(ClaudeSdkAdmissionUnavailable::VersionUnsupported),
            HarnessUnavailableReason::VersionTooOld
        );
        registration.apply_discovery(Some(&InstalledHarness {
            id: "claude-code",
            executable: PathBuf::from("/nonexistent/claude"),
            version: Some("2.1".into()),
        }));
        assert_eq!(
            registration.unavailable(),
            Some(ClaudeSdkAdmissionUnavailable::VersionUnreadable)
        );
        assert_eq!(
            HarnessUnavailableReason::from(ClaudeSdkAdmissionUnavailable::VersionUnreadable),
            HarnessUnavailableReason::CandidateInvalid
        );
    }
}
