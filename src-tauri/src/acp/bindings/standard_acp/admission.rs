use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use sha2::{Digest, Sha256};

use super::profile::{StandardAcpLaunch, StandardAcpProfile};

const BINDING_DIGEST_DOMAIN: &[u8] = b"sage:standard-acp:v1:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StandardAcpArtifact {
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
    pub(crate) byte_len: u64,
    pub(crate) provenance: StandardAcpArtifactProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StandardAcpArtifactProvenance {
    /// A file the user named directly (a user-defined agent).
    Direct,
    NpmGlobal {
        root: PathBuf,
        relative: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StandardAcpCandidate {
    pub(crate) runtime: StandardAcpArtifact,
    pub(crate) entrypoint: Option<StandardAcpArtifact>,
    pub(crate) package_manifest: Option<StandardAcpArtifact>,
    pub(crate) reported_version: String,
}

/// Launch inputs a user-defined agent adds to its profile: its own
/// arguments and the names (never values) of variables passed through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StandardAcpLaunchExtension {
    pub(crate) args: Vec<String>,
    pub(crate) passthrough_env: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct AdmittedStandardAcpBinding {
    profile: &'static StandardAcpProfile,
    /// The id sessions bind to: the profile's for a curated agent, the
    /// agent's own `custom-acp-<n>` for a user-defined one.
    binding_id: String,
    candidate: StandardAcpCandidate,
    extension: StandardAcpLaunchExtension,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
}

impl AdmittedStandardAcpBinding {
    pub(crate) fn profile(&self) -> &'static StandardAcpProfile {
        self.profile
    }

    pub(crate) fn binding_id(&self) -> &str {
        &self.binding_id
    }

    pub(crate) fn candidate(&self) -> &StandardAcpCandidate {
        &self.candidate
    }

    pub(crate) fn extension(&self) -> &StandardAcpLaunchExtension {
        &self.extension
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.binding_digest
    }

    pub(crate) fn binding_digest_hex(&self) -> &str {
        &self.binding_digest_hex
    }

    pub(crate) fn artifact_digest(&self) -> &str {
        &self.candidate.runtime.sha256
    }

    pub(crate) fn harness_version(&self) -> &str {
        &self.candidate.reported_version
    }

    pub(crate) fn revalidate(&self) -> Result<(), StandardAcpAdmissionError> {
        validate_direct_launch(self.profile, &self.candidate.runtime)?;
        validate_artifact(&self.candidate.runtime)?;
        if let Some(entrypoint) = &self.candidate.entrypoint {
            validate_artifact(entrypoint)?;
        }
        validate_package_manifest(
            self.profile,
            self.candidate.package_manifest.as_ref(),
            &self.candidate.reported_version,
        )?;
        validate_package_runtime_link(self.profile, &self.candidate)?;
        Ok(())
    }
}

pub(crate) fn admit(
    profile: &'static StandardAcpProfile,
    candidate: StandardAcpCandidate,
) -> Result<Arc<AdmittedStandardAcpBinding>, StandardAcpAdmissionError> {
    admit_extended(
        profile,
        profile.binding_id,
        candidate,
        StandardAcpLaunchExtension::default(),
    )
}

/// The profile's own id and an empty extension yield exactly the digest [`admit`] gives.
pub(crate) fn admit_extended(
    profile: &'static StandardAcpProfile,
    binding_id: &str,
    candidate: StandardAcpCandidate,
    extension: StandardAcpLaunchExtension,
) -> Result<Arc<AdmittedStandardAcpBinding>, StandardAcpAdmissionError> {
    validate_candidate(profile, &candidate)?;
    let mut digest = Sha256::new()
        .chain_update(BINDING_DIGEST_DOMAIN)
        .chain_update(binding_id.as_bytes())
        .chain_update(candidate.reported_version.as_bytes())
        .chain_update(candidate.runtime.sha256.as_bytes())
        .chain_update(
            candidate
                .entrypoint
                .as_ref()
                .map(|artifact| artifact.sha256.as_bytes())
                .unwrap_or_default(),
        )
        .chain_update(
            candidate
                .package_manifest
                .as_ref()
                .map(|artifact| artifact.sha256.as_bytes())
                .unwrap_or_default(),
        );
    if extension != StandardAcpLaunchExtension::default() {
        for (tag, values) in [(b'a', &extension.args), (b'e', &extension.passthrough_env)] {
            for value in values {
                digest.update([tag]);
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
        }
    }
    let binding_digest: [u8; 32] = digest.finalize().into();
    Ok(Arc::new(AdmittedStandardAcpBinding {
        profile,
        binding_id: binding_id.to_owned(),
        candidate,
        extension,
        binding_digest_hex: binding_digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        binding_digest,
    }))
}

fn validate_candidate(
    profile: &StandardAcpProfile,
    candidate: &StandardAcpCandidate,
) -> Result<(), StandardAcpAdmissionError> {
    if !profile.version_shape.accepts(&candidate.reported_version) {
        return Err(StandardAcpAdmissionError::Version);
    }
    validate_direct_launch(profile, &candidate.runtime)?;
    validate_artifact(&candidate.runtime)?;
    match (profile.launch, candidate.entrypoint.as_ref()) {
        (StandardAcpLaunch::NodeEntrypoint, Some(entrypoint)) => validate_artifact(entrypoint),
        (StandardAcpLaunch::NodeEntrypoint, None) => Err(StandardAcpAdmissionError::Entrypoint),
        (StandardAcpLaunch::DirectExecutable, None) => Ok(()),
        (StandardAcpLaunch::DirectExecutable, Some(_)) => {
            Err(StandardAcpAdmissionError::Entrypoint)
        }
    }?;
    validate_package_manifest(
        profile,
        candidate.package_manifest.as_ref(),
        &candidate.reported_version,
    )?;
    validate_package_runtime_link(profile, candidate)
}

/// On Windows a `.cmd` or `.bat` would run through `cmd.exe`, which interprets the arguments.
/// Node runtimes, their script entrypoints and curated executables found under their package
/// are outside this rule.
fn validate_direct_launch(
    profile: &StandardAcpProfile,
    runtime: &StandardAcpArtifact,
) -> Result<(), StandardAcpAdmissionError> {
    let user_named = profile.launch == StandardAcpLaunch::DirectExecutable
        && runtime.provenance == StandardAcpArtifactProvenance::Direct;
    if user_named && !super::custom_executable::native_executable_name(&runtime.path, cfg!(windows))
    {
        return Err(StandardAcpAdmissionError::Path);
    }
    Ok(())
}

fn validate_package_manifest(
    profile: &StandardAcpProfile,
    package_manifest: Option<&StandardAcpArtifact>,
    reported_version: &str,
) -> Result<(), StandardAcpAdmissionError> {
    match (profile.package_manifest, package_manifest) {
        (None, None) => Ok(()),
        (None, Some(_)) | (Some(_), None) => Err(StandardAcpAdmissionError::Manifest),
        (Some(requirement), Some(artifact)) => {
            validate_artifact_location(artifact)?;
            super::opencode::validate_package_manifest(artifact, requirement, reported_version)
        }
    }
}

fn validate_package_runtime_link(
    profile: &StandardAcpProfile,
    candidate: &StandardAcpCandidate,
) -> Result<(), StandardAcpAdmissionError> {
    let Some(requirement) = profile.package_manifest else {
        return Ok(());
    };
    let Some(manifest) = candidate.package_manifest.as_ref() else {
        return Err(StandardAcpAdmissionError::Manifest);
    };
    let StandardAcpArtifactProvenance::NpmGlobal {
        root: manifest_root,
        ..
    } = &manifest.provenance
    else {
        return Err(StandardAcpAdmissionError::Manifest);
    };
    let expected_relative = super::opencode::runtime_relative(requirement)
        .ok_or(StandardAcpAdmissionError::Manifest)?;
    let StandardAcpArtifactProvenance::NpmGlobal {
        root: runtime_root,
        relative,
    } = &candidate.runtime.provenance
    else {
        return Err(StandardAcpAdmissionError::Manifest);
    };
    (runtime_root == manifest_root
        && relative == &expected_relative
        && candidate.runtime.path == manifest_root.join(expected_relative))
    .then_some(())
    .ok_or(StandardAcpAdmissionError::Manifest)
}

fn validate_artifact(artifact: &StandardAcpArtifact) -> Result<(), StandardAcpAdmissionError> {
    validate_artifact_location(artifact)?;
    if std::fs::metadata(&artifact.path)
        .ok()
        .is_none_or(|metadata| metadata.len() != artifact.byte_len)
        || !valid_sha256(&artifact.sha256)
        || crate::harness_discovery::sha256_file(&artifact.path)
            .is_none_or(|digest| digest != artifact.sha256)
    {
        return Err(StandardAcpAdmissionError::Drift);
    }
    Ok(())
}

fn validate_artifact_location(
    artifact: &StandardAcpArtifact,
) -> Result<(), StandardAcpAdmissionError> {
    let path = &artifact.path;
    validate_path(path)?;
    if path.file_name().is_none() {
        return Err(StandardAcpAdmissionError::Path);
    }
    match &artifact.provenance {
        StandardAcpArtifactProvenance::Direct => {
            crate::harness_discovery::authentic_regular_file(path)
        }
        StandardAcpArtifactProvenance::NpmGlobal { root, relative } => {
            path == &root.join(relative)
                && crate::harness_discovery::authentic_npm_global_descendant(root, relative)
        }
    }
    .then_some(())
    .ok_or(StandardAcpAdmissionError::Path)
}

fn validate_path(path: &Path) -> Result<(), StandardAcpAdmissionError> {
    if !path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(StandardAcpAdmissionError::Path);
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum StandardAcpAdmissionError {
    #[error("standard ACP candidate has an invalid version")]
    Version,
    #[error("standard ACP candidate has an invalid path")]
    Path,
    #[error("standard ACP candidate does not match its launch shape")]
    Entrypoint,
    #[error("standard ACP candidate does not match its package manifest contract")]
    Manifest,
    #[error("standard ACP candidate changed after admission")]
    Drift,
}
