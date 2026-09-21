use std::path::{Component, Path, PathBuf};

pub(crate) const CLAUDE_BINDING_ID: &str = "claude-agent-acp";
pub(crate) const ACP_SDK_VERSION: &str = "1.4.0";
pub(crate) const ENTRYPOINT: &str = "dist/index.js";
pub(crate) const NODE_ENGINE: &str = ">=22";
const MAX_FIXED_ARGUMENTS: usize = 16;
const MAX_ARGUMENT_BYTES: usize = 4_096;
const MAX_ENVIRONMENT_KEYS: usize = 32;

/// Candidate metadata comes only from bounded local discovery. It describes the
/// exact local files Sage will revalidate before every launch; it is not
/// renderer-facing metadata.
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
    pub isolated_state_dir: PathBuf,
    pub fixed_arguments: Vec<String>,
    pub environment_keys: Vec<String>,
    pub project_root_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmittedCandidate {
    pub(crate) adapter_version: String,
    pub(crate) artifact_path: PathBuf,
    pub(crate) runtime_path: PathBuf,
    pub(crate) isolated_state_dir: PathBuf,
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
    forbidden_state_dirs: &[PathBuf],
) -> Result<AdmittedCandidate, ManifestError> {
    validate_absolute_file_path(&candidate.artifact_path)?;
    validate_absolute_file_path(&candidate.runtime_path)?;
    validate_absolute_directory_path(&candidate.isolated_state_dir)?;
    validate_sha256(&candidate.artifact_sha256)?;
    validate_sha256(&candidate.runtime_sha256)?;

    if candidate.adapter_version.is_empty()
        || candidate.acp_sdk_version != ACP_SDK_VERSION
        || candidate.entrypoint != ENTRYPOINT
        || candidate.node_engine != NODE_ENGINE
        || !valid_semver(&candidate.node_version)
    {
        return Err(ManifestError::IdentityMismatch);
    }
    if !node_satisfies_engine(&candidate.node_version) {
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
    if candidate.environment_keys != ["CLAUDE_CONFIG_DIR"]
        || candidate.environment_keys.len() > MAX_ENVIRONMENT_KEYS
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

    Ok(AdmittedCandidate {
        adapter_version: candidate.adapter_version,
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

fn valid_semver(value: &str) -> bool {
    let mut parts = value.split('.');
    !value.is_empty()
        && value.len() <= 256
        && parts.next().is_some_and(valid_semver_component)
        && parts.next().is_some_and(valid_semver_component)
        && parts.next().is_some_and(valid_semver_component)
        && parts.next().is_none()
}

fn valid_semver_component(value: &str) -> bool {
    !value.is_empty() && value.len() <= 10 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn node_satisfies_engine(version: &str) -> bool {
    version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 22)
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
    #[error("Claude candidate identity is incompatible with the admitted ACP launch contract")]
    IdentityMismatch,
    #[error("discovered Node does not satisfy the adapter's declared >=22 engine")]
    NodeEngineMismatch,
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
    #[error("the discovered Claude adapter or Node file changed after admission")]
    Drift,
}
