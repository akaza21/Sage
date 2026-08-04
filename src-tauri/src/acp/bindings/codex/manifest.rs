use std::path::{Path, PathBuf};

use serde::Deserialize;

const MANIFEST_BYTES: usize = 64 * 1024;
const ARTIFACTS_MAX: usize = 64;
const ARTIFACT_BYTES_MAX: u64 = 1024 * 1024 * 1024;
const PATH_BYTES_MAX: usize = 32 * 1024;
const PINNED_CODEX_COMMIT: &str = "c42ea41ee0b836539a2a5d24f3faff96cc48db40";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Platform {
    WindowsX86_64,
    MacosAarch64,
    MacosX86_64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Artifact {
    pub path: PathBuf,
    pub sha256: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CuratedBinding {
    pub schema_version: u8,
    pub binding_id: String,
    pub platform: Platform,
    pub codex_source_commit: String,
    pub codex_runtime_version: String,
    pub app_server_schema_sha256: String,
    pub entrypoint: Artifact,
    pub supporting_artifacts: Vec<Artifact>,
    pub state_policy: StatePolicy,
    pub transport: Transport,
    pub workspace_policy: WorkspacePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StatePolicy {
    IsolatedBindingDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Transport {
    StdioJsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkspacePolicy {
    SingleRootOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedCandidate(CuratedBinding);

impl ValidatedCandidate {
    pub(crate) fn binding(&self) -> &CuratedBinding {
        &self.0
    }

    pub(crate) fn launch_status(&self) -> Result<(), BindingUnavailable> {
        Err(BindingUnavailable::ArtifactNotAdmitted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BindingUnavailable {
    #[error("the exact Codex artifact has not been independently admitted")]
    ArtifactNotAdmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ManifestError {
    #[error("candidate manifest exceeds its byte limit")]
    ManifestBytes,
    #[error("candidate manifest is malformed or has an unknown field")]
    Malformed,
    #[error("candidate manifest identity is invalid")]
    Identity,
    #[error("candidate artifact metadata is invalid")]
    Artifact,
}

pub(crate) fn parse_candidate(bytes: &[u8]) -> Result<ValidatedCandidate, ManifestError> {
    if bytes.len() > MANIFEST_BYTES {
        return Err(ManifestError::ManifestBytes);
    }
    let binding: CuratedBinding =
        serde_json::from_slice(bytes).map_err(|_| ManifestError::Malformed)?;
    validate_binding(&binding)?;
    Ok(ValidatedCandidate(binding))
}

fn validate_binding(binding: &CuratedBinding) -> Result<(), ManifestError> {
    if binding.schema_version != 1
        || binding.binding_id != "codex-app-server"
        || binding.codex_source_commit != PINNED_CODEX_COMMIT
        || !valid_version(&binding.codex_runtime_version)
        || !valid_hex(&binding.app_server_schema_sha256, 64)
        || binding.supporting_artifacts.len() > ARTIFACTS_MAX - 1
    {
        return Err(ManifestError::Identity);
    }

    let mut total = validate_artifact(&binding.entrypoint, binding.platform)?;
    let mut paths = vec![normalized_path_key(
        &binding.entrypoint.path,
        binding.platform,
    )];
    for artifact in &binding.supporting_artifacts {
        total = total
            .checked_add(validate_artifact(artifact, binding.platform)?)
            .ok_or(ManifestError::Artifact)?;
        let key = normalized_path_key(&artifact.path, binding.platform);
        if paths.iter().any(|existing| existing == &key) {
            return Err(ManifestError::Artifact);
        }
        paths.push(key);
    }
    if total > ARTIFACT_BYTES_MAX {
        return Err(ManifestError::Artifact);
    }
    Ok(())
}

fn validate_artifact(artifact: &Artifact, platform: Platform) -> Result<u64, ManifestError> {
    let path = artifact.path.as_os_str().to_string_lossy();
    if path.is_empty()
        || path.len() > PATH_BYTES_MAX
        || artifact.byte_len == 0
        || artifact.byte_len > ARTIFACT_BYTES_MAX
        || !valid_hex(&artifact.sha256, 64)
        || !absolute_normalized(&artifact.path, platform)
    {
        return Err(ManifestError::Artifact);
    }
    Ok(artifact.byte_len)
}

fn absolute_normalized(path: &Path, platform: Platform) -> bool {
    let raw = path.as_os_str().to_string_lossy();
    match platform {
        Platform::WindowsX86_64 => {
            let bytes = raw.as_bytes();
            bytes.len() >= 4
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
                && !raw.contains('/')
                && !raw.split('\\').enumerate().any(|(index, part)| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || (index == 0 && part.len() != 2)
                        || (index > 0
                            && (part.contains(':') || part.ends_with('.') || part.ends_with(' ')))
                })
        }
        Platform::MacosAarch64 | Platform::MacosX86_64 => {
            raw.starts_with('/')
                && !raw
                    .split('/')
                    .skip(1)
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
        }
    }
}

fn normalized_path_key(path: &Path, platform: Platform) -> String {
    let value = path.as_os_str().to_string_lossy().into_owned();
    match platform {
        Platform::WindowsX86_64 => value.to_ascii_lowercase(),
        Platform::MacosAarch64 | Platform::MacosX86_64 => value,
    }
}

fn valid_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}
