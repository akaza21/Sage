use std::{ffi::OsString, path::PathBuf};

use uuid::Uuid;

use crate::{
    acp::{
        bindings::adapter::AdapterRuntimeError,
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec,
            ManifestExpectation, ProcessDeadlines, RequiredContainment, TreeLimits,
        },
    },
    project_registry::ProjectRootAuthority,
};

use super::{AdmittedStandardAcpBinding, StandardAcpLaunch, StandardAcpProfile, profile_for};

pub(crate) struct ClaudeLaunch {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) runtime_epoch: u64,
    pub(crate) runtime_path: PathBuf,
    pub(crate) runtime_sha256: String,
    pub(crate) runtime_byte_len: u64,
    pub(crate) entrypoint: PathBuf,
    pub(crate) environment: BoundedEnvironment,
}

struct LaunchParts {
    binding_digest: [u8; 32],
    runtime_epoch: u64,
    runtime_path: PathBuf,
    runtime_sha256: [u8; 32],
    runtime_byte_len: u64,
    entrypoint: Option<PathBuf>,
    manifest: Option<ManifestExpectation>,
    environment: BoundedEnvironment,
    artifact_identity: [u8; 32],
}

pub(crate) struct AcpLaunchAuthority {
    profile: &'static StandardAcpProfile,
    binding_digest: [u8; 32],
    runtime_epoch: u64,
    runtime_path: PathBuf,
    runtime_sha256: [u8; 32],
    artifact_identity: [u8; 32],
    runtime_byte_len: u64,
    entrypoint: Option<PathBuf>,
    manifest: Option<ManifestExpectation>,
    environment: BoundedEnvironment,
}

impl AcpLaunchAuthority {
    pub(crate) fn for_claude(launch: ClaudeLaunch) -> Result<Self, AdapterRuntimeError> {
        Self::build(
            profile_for("claude-code").ok_or(AdapterRuntimeError::LaunchUnavailable)?,
            LaunchParts {
                binding_digest: launch.binding_digest,
                runtime_epoch: launch.runtime_epoch,
                runtime_path: launch.runtime_path,
                runtime_sha256: parse_digest(&launch.runtime_sha256)?,
                runtime_byte_len: launch.runtime_byte_len,
                entrypoint: Some(launch.entrypoint),
                manifest: None,
                environment: launch.environment,
                artifact_identity: launch.binding_digest,
            },
        )
    }

    fn build(
        profile: &'static StandardAcpProfile,
        parts: LaunchParts,
    ) -> Result<Self, AdapterRuntimeError> {
        let LaunchParts {
            binding_digest,
            runtime_epoch,
            runtime_path,
            runtime_sha256,
            runtime_byte_len,
            entrypoint,
            manifest,
            environment,
            artifact_identity,
        } = parts;
        if !matches!(
            (profile.launch, entrypoint.is_some()),
            (StandardAcpLaunch::NodeEntrypoint, true)
                | (StandardAcpLaunch::DirectExecutable, false)
        ) {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        if profile.package_manifest.is_some() != manifest.is_some() {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        }
        Ok(Self {
            profile,
            binding_digest,
            runtime_epoch,
            runtime_path,
            runtime_sha256,
            artifact_identity,
            runtime_byte_len,
            entrypoint,
            manifest,
            environment,
        })
    }

    pub(crate) fn from_admitted_binding(
        binding: &AdmittedStandardAcpBinding,
        runtime_epoch: u64,
        environment: BoundedEnvironment,
    ) -> Result<Self, AdapterRuntimeError> {
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let candidate = binding.candidate();
        let manifest = match candidate.package_manifest.as_ref() {
            Some(artifact) => Some(
                ManifestExpectation::new(
                    artifact.path.clone(),
                    parse_digest(&artifact.sha256)?,
                    artifact.byte_len,
                )
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            ),
            None => None,
        };
        Self::build(
            binding.profile(),
            LaunchParts {
                binding_digest: binding.binding_digest(),
                runtime_epoch,
                runtime_path: candidate.runtime.path.clone(),
                runtime_sha256: parse_digest(&candidate.runtime.sha256)?,
                runtime_byte_len: candidate.runtime.byte_len,
                entrypoint: candidate
                    .entrypoint
                    .as_ref()
                    .map(|artifact| artifact.path.clone()),
                manifest,
                environment,
                artifact_identity: parse_digest(&candidate.runtime.sha256)?,
            },
        )
    }

    pub(crate) fn into_launch_spec(
        self,
        project_root: ProjectRootAuthority,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        let fixed_args = BoundedOsArgs::new(self.fixed_args())
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let artifact = ArtifactExpectation::new(
            digest_identity(self.artifact_identity),
            self.runtime_path,
            self.runtime_sha256,
            self.runtime_byte_len,
        )
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?
        .with_manifest(self.manifest);
        project_root
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(LaunchSpec {
            binding_id: digest_identity(self.binding_digest),
            runtime_epoch: self.runtime_epoch,
            artifact,
            fixed_args,
            environment: self.environment,
            project_root,
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

    fn fixed_args(&self) -> Vec<OsString> {
        let mut fixed_args = Vec::with_capacity(self.profile.fixed_args.len().saturating_add(1));
        if let Some(entrypoint) = &self.entrypoint {
            fixed_args.push(OsString::from(entrypoint));
        }
        fixed_args.extend(self.profile.fixed_args.iter().map(OsString::from));
        fixed_args
    }
}

fn parse_digest(value: &str) -> Result<[u8; 32], AdapterRuntimeError> {
    let value = value.as_bytes();
    if value.len() != 64 || !value.iter().all(u8::is_ascii_hexdigit) {
        return Err(AdapterRuntimeError::LaunchUnavailable);
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = (hex_nibble(value[index * 2])? << 4) | hex_nibble(value[index * 2 + 1])?;
    }
    Ok(digest)
}

fn hex_nibble(value: u8) -> Result<u8, AdapterRuntimeError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(AdapterRuntimeError::LaunchUnavailable),
    }
}

fn digest_identity(digest: [u8; 32]) -> Uuid {
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(identity)
}
