use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    acp::{
        contract::{
            ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
            SageRootId,
        },
        limits::PREVIEW_LIMITS,
    },
    project_registry::RootRecord,
};

use super::error::PreviewError;
#[cfg(test)]
use super::lifecycle;
use crate::acp::package_manifest::{PackageManifestError, read_bounded_package_manifest};

const MAX_DEPTH: usize = PREVIEW_LIMITS.discovery_depth as usize;
const MAX_PACKAGES_PER_ROOT: usize = PREVIEW_LIMITS.packages_per_root as usize;
const MAX_TARGETS_PER_PROJECT: usize = PREVIEW_LIMITS.targets_per_project as usize;
const MAX_DIRECTORIES_PER_ROOT: usize = PREVIEW_LIMITS.directories_per_root as usize;
const MAX_DIRECTORY_ENTRIES: usize = PREVIEW_LIMITS.directory_entries as usize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewDiscoveryRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub expected_transition_epoch: u64,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewTargetRef {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub expected_transition_epoch: u64,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub root_id: SageRootId,
    pub script_name: PreviewScriptName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PreviewScriptName {
    Dev,
    Start,
}

impl PreviewScriptName {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Start => "start",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewDiscoveryResponse {
    pub project_id: SageProjectId,
    pub transition_epoch: u64,
    pub targets: Vec<PreviewTargetRef>,
    pub truncated: bool,
}

pub(crate) fn validate_discovery_request(
    request: &PreviewDiscoveryRequest,
) -> Result<(), PreviewError> {
    if request.expected_transition_epoch > PREVIEW_LIMITS.transition_epoch_max
        || request.project_generation.get() != request.expected_transition_epoch
    {
        return Err(PreviewError::invalid(
            "Preview request identity is invalid.",
        ));
    }
    Ok(())
}

pub(crate) fn validate_target(target: &PreviewTargetRef) -> Result<(), PreviewError> {
    validate_discovery_request(&PreviewDiscoveryRequest {
        project_id: target.project_id.clone(),
        project_generation: target.project_generation,
        expected_transition_epoch: target.expected_transition_epoch,
        chat_id: target.chat_id.clone(),
        session_id: target.session_id.clone(),
        runtime_epoch: target.runtime_epoch,
    })?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn discover(
    request: &PreviewDiscoveryRequest,
    roots: Vec<RootRecord>,
) -> Result<PreviewDiscoveryResponse, PreviewError> {
    let (response, mappings) = discover_uninstalled(request, roots)?;
    lifecycle::replace_discovered(request, mappings);
    Ok(response)
}

pub(crate) fn discover_uninstalled(
    request: &PreviewDiscoveryRequest,
    roots: Vec<RootRecord>,
) -> Result<(PreviewDiscoveryResponse, Vec<(PreviewTargetRef, PathBuf)>), PreviewError> {
    let mut targets = Vec::new();
    let mut mappings = Vec::new();
    let mut truncated = false;
    'roots: for root in roots {
        if targets.len() == MAX_TARGETS_PER_PROJECT {
            truncated = true;
            break;
        }
        let (packages, root_truncated) = discover_root(&root.absolute_path)?;
        truncated |= root_truncated;
        let mut scripts = BTreeSet::new();
        for package in packages {
            if targets.len() == MAX_TARGETS_PER_PROJECT {
                truncated = true;
                break 'roots;
            }
            let package_scripts = match package_scripts(&package) {
                Ok(scripts) => scripts,
                Err(_) => {
                    truncated = true;
                    continue;
                }
            };
            for script_name in package_scripts {
                if !scripts.insert(script_name) {
                    continue;
                }
                if targets.len() == MAX_TARGETS_PER_PROJECT {
                    truncated = true;
                    break 'roots;
                }
                let target = PreviewTargetRef {
                    project_id: request.project_id.clone(),
                    project_generation: request.project_generation,
                    expected_transition_epoch: request.expected_transition_epoch,
                    chat_id: request.chat_id.clone(),
                    session_id: request.session_id.clone(),
                    runtime_epoch: request.runtime_epoch,
                    root_id: SageRootId::parse(root.id.clone())
                        .map_err(|_| PreviewError::internal("Preview root identity is invalid."))?,
                    script_name,
                };
                let Some(package_root) = package.parent() else {
                    truncated = true;
                    continue;
                };
                mappings.push((target.clone(), package_root.to_owned()));
                targets.push(target);
            }
        }
    }
    Ok((
        PreviewDiscoveryResponse {
            project_id: request.project_id.clone(),
            transition_epoch: request.expected_transition_epoch,
            targets,
            truncated,
        },
        mappings,
    ))
}

fn discover_root(root: &Path) -> Result<(Vec<PathBuf>, bool), PreviewError> {
    let mut packages = Vec::new();
    let mut truncated = false;
    // The root package has priority independently of bounded enumeration.
    // Only regular files enter the existing bounded, reparse-aware reader.
    let root_package = root.join("package.json");
    match fs::symlink_metadata(&root_package) {
        Ok(metadata) if metadata.is_file() => packages.push(root_package),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => truncated = true,
    }
    let mut pending = vec![(root.to_owned(), 0_usize)];
    let mut admitted_directories = 1_usize;
    let mut visited = 0_usize;
    'directories: while let Some((directory, depth)) = pending.pop() {
        if packages.len() == MAX_PACKAGES_PER_ROOT {
            truncated = true;
            break;
        }
        if visited == MAX_DIRECTORIES_PER_ROOT {
            truncated = true;
            break;
        }
        visited += 1;
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        let mut ordered_entries = Vec::new();
        for (entry_count, entry) in entries.take(MAX_DIRECTORY_ENTRIES + 1).enumerate() {
            if entry_count == MAX_DIRECTORY_ENTRIES {
                // A raw read_dir prefix is platform dependent. Discard this
                // entire listing rather than choosing an arbitrary subset.
                truncated = true;
                continue 'directories;
            }
            let Ok(entry) = entry else {
                truncated = true;
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            ordered_entries.push((name.to_owned(), entry));
        }
        // UTF-8 byte order is platform independent for the admitted names.
        ordered_entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let pending_start = pending.len();
        for (name, entry) in ordered_entries {
            if packages.len() == MAX_PACKAGES_PER_ROOT {
                truncated = true;
                break;
            }
            let file_type = match entry.file_type() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if is_reparse_point(&entry, &file_type) {
                continue;
            }
            if depth > 0 && file_type.is_file() && name == "package.json" {
                packages.push(entry.path());
            } else if file_type.is_dir() && depth < MAX_DEPTH && !skipped_directory(&name) {
                if admitted_directories == MAX_DIRECTORIES_PER_ROOT {
                    truncated = true;
                } else {
                    admitted_directories += 1;
                    pending.push((entry.path(), depth + 1));
                }
            }
        }
        // The stack pops the smallest sibling first, after this directory's package.
        pending[pending_start..].reverse();
    }
    Ok((packages, truncated))
}

fn skipped_directory(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | ".git" | ".hg" | ".svn" | ".yarn" | ".pnpm-store"
    )
}

#[cfg(windows)]
fn is_reparse_point(entry: &fs::DirEntry, file_type: &fs::FileType) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    file_type.is_symlink()
        || entry
            .metadata()
            .map(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
            .unwrap_or(true)
}

#[cfg(not(windows))]
fn is_reparse_point(_entry: &fs::DirEntry, file_type: &fs::FileType) -> bool {
    file_type.is_symlink()
}

fn package_scripts(path: &Path) -> Result<Vec<PreviewScriptName>, PreviewError> {
    let Some(value) = read_bounded_package_manifest(path).map_err(manifest_error)? else {
        return Ok(Vec::new());
    };
    let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for (name, script) in [
        ("dev", PreviewScriptName::Dev),
        ("start", PreviewScriptName::Start),
    ] {
        if scripts
            .get(name)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| {
                !value.trim().is_empty() && value.len() <= PREVIEW_LIMITS.script_bytes as usize
            })
        {
            result.push(script);
        }
    }
    Ok(result)
}

fn manifest_error(error: PackageManifestError) -> PreviewError {
    match error {
        PackageManifestError::Open => {
            PreviewError::internal("Preview package manifest could not be opened.")
        }
        PackageManifestError::Metadata => {
            PreviewError::internal("Preview package metadata could not be read.")
        }
        PackageManifestError::Read => {
            PreviewError::internal("Preview package manifest could not be read.")
        }
        PackageManifestError::Malformed => {
            PreviewError::invalid("Preview package manifest is invalid.")
        }
    }
}

#[cfg(test)]
pub(super) fn test_depth_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.discovery_depth
}

#[cfg(test)]
pub(super) fn test_package_count_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.packages_per_root
}

#[cfg(test)]
pub(super) fn test_directory_count_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.directories_per_root
}

#[cfg(test)]
pub(super) fn test_manifest_bytes_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.manifest_bytes
}

#[cfg(test)]
pub(super) fn test_target_count_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.targets_per_project
}

#[cfg(test)]
pub(super) fn test_directory_entries_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.directory_entries
}

#[cfg(test)]
pub(super) fn test_script_bytes_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.script_bytes
}
