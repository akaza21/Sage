use std::path::{Component, Path, PathBuf};

const SOURCE_COMMIT: &str = "c2e4815029ef3962787ecaefe208b0b6f8b81302";
const ADAPTER_VERSION: &str = "0.76.0";
const ACP_SDK_VERSION: &str = "1.4.0";
const CLAUDE_SDK_VERSION: &str = "0.3.257";
const ZOD_VERSION: &str = "4.6.4";
const ENTRYPOINT: &str = "dist/index.js";
const NODE_ENGINE: &str = ">=22";
const NODE_VERSION: &str = "22.18.0";
const ARTIFACT_SHA256: &str = "9d73d1f0f121fb96cc8badb28c22d5bff02d8582eb2e40360a81c189e1b9422a";
const RUNTIME_SHA256: &str = "c22d1c59a1f767a1ed0178445a027f2257d318c55430fc819d48f269586822b7";
const LICENSE: &str = "Apache-2.0";
const MAX_FIXED_ARGUMENTS: usize = 16;
const MAX_ARGUMENT_BYTES: usize = 4_096;
const MAX_ENVIRONMENT_KEYS: usize = 32;

/// Candidate metadata may be inspected and tested, but never authorizes launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CandidateManifest {
    pub source_commit: Option<String>,
    pub adapter_version: String,
    pub acp_sdk_version: String,
    pub claude_sdk_version: String,
    pub zod_version: String,
    pub entrypoint: String,
    pub node_engine: String,
    pub node_version: String,
    pub license: String,
    pub artifact_path: PathBuf,
    /// SHA-256 of the exact executable adapter entrypoint at `artifact_path`.
    pub artifact_sha256: String,
    pub runtime_path: PathBuf,
    /// SHA-256 of the exact Node executable at `runtime_path`.
    pub runtime_sha256: String,
    pub isolated_state_dir: PathBuf,
    pub fixed_arguments: Vec<String>,
    pub environment_keys: Vec<String>,
    pub project_root_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResearchCandidate {
    pub artifact_path: PathBuf,
    pub runtime_path: PathBuf,
    pub isolated_state_dir: PathBuf,
    pub artifact_sha256: String,
    pub runtime_sha256: String,
}

impl ResearchCandidate {
    /// H3 remains research-only until its complete immutable runtime graph,
    /// authority, and platform evidence are admitted outside this packet.
    pub(crate) fn require_launch_admission(&self) -> Result<(), ManifestError> {
        Err(ManifestError::ArtifactGraphNotAdmitted)
    }
}

pub(crate) fn validate_research_candidate(
    candidate: CandidateManifest,
    forbidden_state_dirs: &[PathBuf],
) -> Result<ResearchCandidate, ManifestError> {
    validate_absolute_file_path(&candidate.artifact_path)?;
    validate_absolute_file_path(&candidate.runtime_path)?;
    validate_absolute_directory_path(&candidate.isolated_state_dir)?;
    validate_sha256(&candidate.artifact_sha256)?;
    validate_sha256(&candidate.runtime_sha256)?;

    if candidate.source_commit.as_deref() != Some(SOURCE_COMMIT)
        || candidate.adapter_version != ADAPTER_VERSION
        || candidate.acp_sdk_version != ACP_SDK_VERSION
        || candidate.claude_sdk_version != CLAUDE_SDK_VERSION
        || candidate.zod_version != ZOD_VERSION
        || candidate.entrypoint != ENTRYPOINT
        || candidate.node_engine != NODE_ENGINE
        || candidate.node_version != NODE_VERSION
        || candidate.license != LICENSE
        || candidate.artifact_sha256 != ARTIFACT_SHA256
        || candidate.runtime_sha256 != RUNTIME_SHA256
    {
        return Err(ManifestError::IdentityMismatch);
    }

    if candidate.fixed_arguments.len() > MAX_FIXED_ARGUMENTS
        || candidate.fixed_arguments.iter().any(|argument| {
            argument.is_empty() || argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0')
        })
    {
        return Err(ManifestError::Arguments);
    }
    if candidate.environment_keys.len() > MAX_ENVIRONMENT_KEYS
        || candidate.environment_keys.iter().any(|key| {
            key.is_empty()
                || key.len() > 128
                || key.contains('=')
                || key.contains('\0')
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        })
    {
        return Err(ManifestError::Environment);
    }
    if candidate.project_root_count != 1 {
        return Err(ManifestError::MultiRootUnsupported);
    }
    if forbidden_state_dirs
        .iter()
        .any(|path| same_path(path, &candidate.isolated_state_dir))
    {
        return Err(ManifestError::AmbientState);
    }

    Ok(ResearchCandidate {
        artifact_path: candidate.artifact_path,
        runtime_path: candidate.runtime_path,
        isolated_state_dir: candidate.isolated_state_dir,
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

fn validate_absolute_directory_path(path: &Path) -> Result<(), ManifestError> {
    validate_absolute_path(path)
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

#[cfg(windows)]
fn same_path(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(not(windows))]
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ManifestError {
    #[error("Claude candidate identity does not match the pinned source ledger")]
    IdentityMismatch,
    #[error("Claude candidate uses a noncanonical or nonabsolute path")]
    Path,
    #[error("Claude candidate hash is not canonical lowercase SHA-256")]
    Hash,
    #[error("Claude fixed arguments exceed the admitted bounds")]
    Arguments,
    #[error("Claude environment allowlist is invalid or over limit")]
    Environment,
    #[error("Claude candidate attempted to use ambient or normal state")]
    AmbientState,
    #[error("Claude candidate does not support this multi-root project")]
    MultiRootUnsupported,
    #[error("Claude runtime artifact graph is research-only and not admitted")]
    ArtifactGraphNotAdmitted,
}
