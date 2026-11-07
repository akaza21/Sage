//! Production Pi admission.
//!
//! Curated installation metadata admits the npm-global
//! `@earendil-works/pi-coding-agent` package through Node over its JSONL RPC
//! mode, within a bounded version range whose wire behaviour the pinned
//! translator covers. Launch is read-only by construction: Pi is started with
//! only its `read` tool, no extensions, skills, prompt templates, themes or
//! session files. An exact Sage-bundled bridge may explicitly register managed
//! tools on the pinned release; missing or stale resources leave only read.

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
    domain::HarnessCharacterisation,
    harness_discovery::{DiscoveredPiLaunch, PiLaunchDiscovery},
};

use super::manifest::parse_version;

pub(crate) const PI_BINDING_ID: &str = "pi-rpc";
/// Oldest Pi release whose RPC commands and events the translator covers.
pub(crate) const PI_SUPPORTED_MINIMUM: (u64, u64, u64) = (0, 74, 0);
const BINDING_DIGEST_DOMAIN: &[u8] = b"sage:pi-rpc:native:v1:";
/// Read-only launch arguments after the entrypoint.
pub(crate) const PI_READ_ONLY_ARGS: [&str; 9] = [
    "--mode",
    "rpc",
    "--tools",
    "read",
    "--no-extensions",
    "--no-skills",
    "--no-prompt-templates",
    "--no-themes",
    "--no-session",
];
/// Launch arguments with the pinned bridge. Pi's `--tools` allowlist also
/// filters extension tools, so the bridge's managed write, edit and bash would
/// never register under `--tools read`. Built-in tools instead start disabled;
/// the bridge then activates only built-in `read` beside its own tools, and a
/// bridge that fails to load leaves Pi with no tools at all.
const PI_BRIDGE_ARGS: [&str; 8] = [
    "--mode",
    "rpc",
    "--no-builtin-tools",
    "--no-extensions",
    "--no-skills",
    "--no-prompt-templates",
    "--no-themes",
    "--no-session",
];

pub(crate) fn pi_launch_args(bridge: Option<PathBuf>) -> Vec<OsString> {
    let Some(bridge) = bridge else {
        return PI_READ_ONLY_ARGS.map(OsString::from).into();
    };
    let mut args: Vec<OsString> = PI_BRIDGE_ARGS.map(OsString::from).into();
    args.extend([OsString::from("--extension"), bridge.into_os_string()]);
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PiAdmissionUnavailable {
    #[error("Pi discovery is still in progress")]
    DiscoveryPending,
    #[error("Pi is not installed")]
    Absent,
    #[error("the Pi package or its version could not be read")]
    PackageUnreadable,
    #[error("Node.js required by Pi is not installed")]
    NodeAbsent,
    #[error("the discovered Node.js version could not be read")]
    NodeVersionUnreadable,
    #[error("the discovered Node.js does not satisfy Pi's engine")]
    NodeEngineMismatch,
    #[error("the installed Pi version is outside the curated range")]
    VersionUnsupported,
    #[error("the discovered Pi package or Node runtime changed after admission")]
    Drift,
}

#[derive(Debug)]
pub(crate) struct AdmittedPiBinding {
    launch: DiscoveredPiLaunch,
    node_byte_len: u64,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn engine_satisfied(engine: &str, node_version: &str) -> Option<bool> {
    let minimum = parse_version(engine.trim().strip_prefix(">=")?.trim())?;
    Some(parse_version(node_version)? >= minimum)
}

/// Curated version range: at least [`PI_SUPPORTED_MINIMUM`], and no newer
/// minor than the pinned [`super::PI_VERSION`].
pub(crate) fn version_admitted(version: &str) -> bool {
    let (Some(found), Some(pinned)) = (parse_version(version), parse_version(super::PI_VERSION))
    else {
        return false;
    };
    found >= PI_SUPPORTED_MINIMUM && (found.0, found.1) <= (pinned.0, pinned.1)
}

impl AdmittedPiBinding {
    fn admit(launch: DiscoveredPiLaunch) -> Result<Self, PiAdmissionUnavailable> {
        if !version_admitted(&launch.package_version) {
            return Err(PiAdmissionUnavailable::VersionUnsupported);
        }
        match engine_satisfied(&launch.node_engine, &launch.node_version) {
            Some(true) => {}
            Some(false) => return Err(PiAdmissionUnavailable::NodeEngineMismatch),
            None => return Err(PiAdmissionUnavailable::PackageUnreadable),
        }
        let node_byte_len = std::fs::metadata(&launch.node_path)
            .map_err(|_| PiAdmissionUnavailable::NodeVersionUnreadable)?
            .len();
        let binding_digest: [u8; 32] = Sha256::new()
            .chain_update(BINDING_DIGEST_DOMAIN)
            .chain_update(super::bridge::install::SOURCE)
            .chain_update(launch.package_version.as_bytes())
            .chain_update(launch.entrypoint_sha256.as_bytes())
            .chain_update(launch.node_sha256.as_bytes())
            .finalize()
            .into();
        Ok(Self {
            launch,
            node_byte_len,
            binding_digest_hex: hex(&binding_digest),
            binding_digest,
        })
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.binding_digest
    }
    pub(crate) fn binding_digest_hex(&self) -> &str {
        &self.binding_digest_hex
    }
    pub(crate) fn artifact_digest(&self) -> &str {
        &self.launch.entrypoint_sha256
    }
    pub(crate) fn harness_version(&self) -> &str {
        &self.launch.package_version
    }
    pub(crate) fn characterisation(&self) -> HarnessCharacterisation {
        if self.launch.package_version == super::PI_VERSION {
            HarnessCharacterisation::Characterised
        } else {
            HarnessCharacterisation::Uncharacterised
        }
    }

    /// Revalidates both launch fingerprints through the shared metadata-keyed hash cache.
    pub(crate) fn revalidate(&self) -> Result<(), PiAdmissionUnavailable> {
        let same = |path: &PathBuf, expected: &str| {
            crate::harness_discovery::cached_sha256_file(path).as_deref() == Some(expected)
        };
        if same(&self.launch.entrypoint_path, &self.launch.entrypoint_sha256)
            && same(&self.launch.node_path, &self.launch.node_sha256)
        {
            Ok(())
        } else {
            Err(PiAdmissionUnavailable::Drift)
        }
    }

    pub(crate) fn launch_spec(
        &self,
        epoch: u64,
        root: crate::project_registry::ProjectRootAuthority,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        self.revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        root.revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let mut node_sha256 = [0_u8; 32];
        for (index, byte) in node_sha256.iter_mut().enumerate() {
            *byte = u8::from_str_radix(
                self.launch
                    .node_sha256
                    .get(index * 2..index * 2 + 2)
                    .ok_or(AdapterRuntimeError::LaunchUnavailable)?,
                16,
            )
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        }
        let mut identity = [0_u8; 16];
        identity.copy_from_slice(&self.binding_digest[..16]);
        let identity = uuid::Uuid::from_bytes(identity);
        let mut fixed_args = vec![self.launch.entrypoint_path.clone().into_os_string()];
        fixed_args.extend(pi_launch_args(
            super::bridge::install::installed(self.harness_version()).ok(),
        ));
        Ok(LaunchSpec {
            binding_id: identity,
            runtime_epoch: epoch,
            artifact: ArtifactExpectation::new(
                identity,
                self.launch.node_path.clone(),
                node_sha256,
                self.node_byte_len,
            )
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            fixed_args: BoundedOsArgs::new(fixed_args)
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            environment: BoundedEnvironment::new(launch_environment())
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
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

/// Only the variables Pi needs to find its own `~/.pi` profile and Node.
fn launch_environment() -> Vec<(OsString, OsString)> {
    let keys: &[&str] = if cfg!(windows) {
        &[
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "PATH",
            "PATHEXT",
            "ComSpec",
            "SystemRoot",
            "TEMP",
            "TMP",
        ]
    } else {
        &["HOME", "PATH", "TMPDIR"]
    };
    keys.iter()
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
    Unavailable(PiAdmissionUnavailable),
    Admitted(Arc<AdmittedPiBinding>),
}

#[derive(Debug)]
pub(crate) struct PiBindingRegistration {
    state: Mutex<RegistrationState>,
}

impl PiBindingRegistration {
    pub(crate) fn startup() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Pending),
        }
    }

    pub(crate) fn apply_discovery(&self, discovery: PiLaunchDiscovery) {
        *self.lock_state() = match discovery {
            PiLaunchDiscovery::Absent => {
                RegistrationState::Unavailable(PiAdmissionUnavailable::Absent)
            }
            PiLaunchDiscovery::PackageUnreadable => {
                RegistrationState::Unavailable(PiAdmissionUnavailable::PackageUnreadable)
            }
            PiLaunchDiscovery::NodeAbsent => {
                RegistrationState::Unavailable(PiAdmissionUnavailable::NodeAbsent)
            }
            PiLaunchDiscovery::NodeVersionUnreadable => {
                RegistrationState::Unavailable(PiAdmissionUnavailable::NodeVersionUnreadable)
            }
            PiLaunchDiscovery::Present(launch) => match AdmittedPiBinding::admit(*launch) {
                Ok(binding) => RegistrationState::Admitted(Arc::new(binding)),
                Err(reason) => RegistrationState::Unavailable(reason),
            },
        };
    }

    pub(crate) fn refresh(&self) -> Result<Arc<AdmittedPiBinding>, PiAdmissionUnavailable> {
        let binding = self.admitted().ok_or_else(|| {
            self.unavailable()
                .unwrap_or(PiAdmissionUnavailable::DiscoveryPending)
        })?;
        let validation = binding.revalidate();
        let mut state = self.lock_state();
        match (&*state, validation) {
            (RegistrationState::Admitted(current), Ok(())) if Arc::ptr_eq(current, &binding) => {
                Ok(binding)
            }
            (RegistrationState::Admitted(current), Err(reason))
                if Arc::ptr_eq(current, &binding) =>
            {
                *state = RegistrationState::Unavailable(reason);
                Err(reason)
            }
            _ => Err(PiAdmissionUnavailable::Drift),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Admitted(_))
    }

    pub(crate) fn admitted(&self) -> Option<Arc<AdmittedPiBinding>> {
        match &*self.lock_state() {
            RegistrationState::Admitted(binding) => Some(Arc::clone(binding)),
            RegistrationState::Pending | RegistrationState::Unavailable(_) => None,
        }
    }

    pub(crate) fn unavailable(&self) -> Option<PiAdmissionUnavailable> {
        match *self.lock_state() {
            RegistrationState::Pending => Some(PiAdmissionUnavailable::DiscoveryPending),
            RegistrationState::Unavailable(reason) => Some(reason),
            RegistrationState::Admitted(_) => None,
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, RegistrationState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(crate) fn from_discovery_for_test(discovery: PiLaunchDiscovery) -> Self {
        let registration = Self::startup();
        registration.apply_discovery(discovery);
        registration
    }
}

#[cfg(test)]
#[path = "native_admission_tests.rs"]
mod tests;
