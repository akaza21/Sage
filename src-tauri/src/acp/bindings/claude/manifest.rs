use std::path::{Component, Path, PathBuf};

use crate::acp::bindings::characterisation::{
    Characterisation, claude_characterisation, node_satisfies_declared_engine,
    valid_numeric_version,
};

pub(crate) const CLAUDE_BINDING_ID: &str = "claude-agent-acp";
pub(crate) const ENTRYPOINT: &str = "dist/index.js";
#[cfg(windows)]
pub(crate) const LAUNCH_ENVIRONMENT_KEYS: [&str; 10] = [
    "CLAUDE_CONFIG_DIR",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PATH",
    "PATHEXT",
    "ComSpec",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
];
#[cfg(any(not(windows), test))]
pub(crate) const UNIX_LAUNCH_ENVIRONMENT_KEYS: [&str; 4] =
    ["CLAUDE_CONFIG_DIR", "HOME", "PATH", "TMPDIR"];
#[cfg(not(windows))]
pub(crate) const LAUNCH_ENVIRONMENT_KEYS: [&str; 4] = UNIX_LAUNCH_ENVIRONMENT_KEYS;
const MAX_FIXED_ARGUMENTS: usize = 16;
const MAX_ARGUMENT_BYTES: usize = 4_096;
const MAX_ENVIRONMENT_KEYS: usize = 32;

/// Candidate metadata describes the exact local files Sage revalidates before every launch; it is
/// not renderer-facing metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CandidateManifest {
    pub adapter_version: String,
    pub acp_sdk_version: String,
    pub entrypoint: String,
    pub node_engine: String,
    pub node_version: String,
    pub artifact_path: PathBuf,
    pub artifact_sha256: String,
    pub runtime_path: PathBuf,
    pub runtime_sha256: String,
    pub fixed_arguments: Vec<String>,
    pub environment_keys: Vec<String>,
    pub project_root_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmittedCandidate {
    pub(crate) adapter_version: String,
    pub(crate) characterisation: Characterisation,
    pub(crate) artifact_path: PathBuf,
    pub(crate) runtime_path: PathBuf,
    pub(crate) artifact_sha256: String,
    pub(crate) runtime_sha256: String,
}

impl AdmittedCandidate {
    pub(crate) fn require_launch_admission(&self) -> Result<(), ManifestError> {
        if !crate::harness_discovery::sha256_file(&self.artifact_path)
            .is_some_and(|digest| digest == self.artifact_sha256)
            || !crate::harness_discovery::sha256_file(&self.runtime_path)
                .is_some_and(|digest| digest == self.runtime_sha256)
        {
            return Err(ManifestError::Drift);
        }
        Ok(())
    }
}

pub(crate) fn validate_research_candidate(
    candidate: CandidateManifest,
) -> Result<AdmittedCandidate, ManifestError> {
    validate_absolute_file_path(&candidate.artifact_path)?;
    validate_absolute_file_path(&candidate.runtime_path)?;
    validate_sha256(&candidate.artifact_sha256)?;
    validate_sha256(&candidate.runtime_sha256)?;

    if candidate.adapter_version.is_empty()
        || candidate.entrypoint != ENTRYPOINT
        || candidate.acp_sdk_version.is_empty()
        || candidate.adapter_version.len() > 64
        || candidate.acp_sdk_version.len() > 64
        || !valid_semver(&candidate.node_version)
    {
        return Err(ManifestError::IdentityMismatch);
    }
    if !node_satisfies_declared_engine(&candidate.node_version, &candidate.node_engine) {
        return Err(ManifestError::NodeEngineMismatch);
    }
    if candidate.fixed_arguments.len() > MAX_FIXED_ARGUMENTS
        || candidate.fixed_arguments != [candidate.artifact_path.to_string_lossy()]
        || candidate.fixed_arguments.iter().any(|argument| {
            argument.is_empty() || argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0')
        })
    {
        return Err(ManifestError::Arguments);
    }
    if !candidate
        .environment_keys
        .iter()
        .map(String::as_str)
        .eq(LAUNCH_ENVIRONMENT_KEYS.iter().copied())
        || candidate.environment_keys.len() > MAX_ENVIRONMENT_KEYS
        || candidate.environment_keys.iter().any(|key| {
            key.is_empty()
                || key.len() > 128
                || key.contains('=')
                || key.contains('\0')
                || !(key == "ComSpec"
                    || key
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte == b'_'))
        })
    {
        return Err(ManifestError::Environment);
    }
    if candidate.project_root_count != 1 {
        return Err(ManifestError::MultiRootUnsupported);
    }
    Ok(AdmittedCandidate {
        characterisation: claude_characterisation(
            &candidate.adapter_version,
            &candidate.acp_sdk_version,
        ),
        adapter_version: candidate.adapter_version,
        artifact_path: candidate.artifact_path,
        runtime_path: candidate.runtime_path,
        artifact_sha256: candidate.artifact_sha256,
        runtime_sha256: candidate.runtime_sha256,
    })
}

fn validate_absolute_file_path(path: &Path) -> Result<(), ManifestError> {
    validate_absolute_path(path)?;
    if path.file_name().is_none() {
        return Err(ManifestError::Path);
    }
    Ok(())
}

fn validate_absolute_path(path: &Path) -> Result<(), ManifestError> {
    if !path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(ManifestError::Path);
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), ManifestError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ManifestError::Hash);
    }
    Ok(())
}

fn valid_semver(value: &str) -> bool {
    value.len() <= 256 && valid_numeric_version(value, 3, 3)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ManifestError {
    #[error("Claude candidate identity is incompatible with the admitted ACP launch contract")]
    IdentityMismatch,
    #[error("discovered Node does not satisfy the adapter's declared engine")]
    NodeEngineMismatch,
    #[error("Claude candidate uses a noncanonical or nonabsolute path")]
    Path,
    #[error("Claude candidate hash is not canonical lowercase SHA-256")]
    Hash,
    #[error("Claude fixed arguments exceed the admitted bounds")]
    Arguments,
    #[error("Claude environment allowlist is invalid or over limit")]
    Environment,
    #[error("Claude candidate does not support this multi-root project")]
    MultiRootUnsupported,
    #[error("the discovered Claude adapter or Node file changed after admission")]
    Drift,
}
