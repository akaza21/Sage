use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::project_registry::RootRecord;

use super::{error::PreviewError, lifecycle};

const MAX_DEPTH: usize = 4;
const MAX_PACKAGES_PER_ROOT: usize = 8;
const MAX_TARGETS_PER_PROJECT: usize = 16;
const MAX_DIRECTORIES_PER_ROOT: usize = 256;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024;
const MAX_SAFE_JAVASCRIPT_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewDiscoveryRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewTargetRef {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub root_id: String,
    pub script_name: PreviewScriptName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    pub project_id: String,
    pub transition_epoch: u64,
    pub targets: Vec<PreviewTargetRef>,
    pub truncated: bool,
}

pub(crate) fn validate_discovery_request(
    request: &PreviewDiscoveryRequest,
) -> Result<(), PreviewError> {
    if !valid_identifier(&request.project_id)
        || request.project_generation > MAX_SAFE_JAVASCRIPT_INTEGER
        || request.expected_transition_epoch > MAX_SAFE_JAVASCRIPT_INTEGER
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
    })?;
    if !valid_identifier(&target.root_id) {
        return Err(PreviewError::invalid("Preview target root is invalid."));
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains('\0')
        && !value.chars().any(char::is_control)
}

pub(crate) fn discover(
    request: &PreviewDiscoveryRequest,
    roots: Vec<RootRecord>,
) -> Result<PreviewDiscoveryResponse, PreviewError> {
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
            for script_name in package_scripts(&package)? {
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
                    root_id: root.id.clone(),
                    script_name,
                };
                mappings.push((
                    target.clone(),
                    package
                        .parent()
                        .expect("package file has parent")
                        .to_owned(),
                ));
                targets.push(target);
                if targets.len() == MAX_TARGETS_PER_PROJECT {
                    truncated = true;
                    break 'roots;
                }
            }
        }
    }
    lifecycle::replace_discovered(request, mappings);
    Ok(PreviewDiscoveryResponse {
        project_id: request.project_id.clone(),
        transition_epoch: request.expected_transition_epoch,
        targets,
        truncated,
    })
}

fn discover_root(root: &Path) -> Result<(Vec<PathBuf>, bool), PreviewError> {
    let mut packages = Vec::new();
    let mut pending = vec![(root.to_owned(), 0_usize)];
    let mut visited = 0_usize;
    let mut truncated = false;
    while let Some((directory, depth)) = pending.pop() {
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
        let mut entry_count = 0_usize;
        for entry in entries.take(MAX_DIRECTORIES_PER_ROOT) {
            entry_count += 1;
            if packages.len() == MAX_PACKAGES_PER_ROOT {
                truncated = true;
                break;
            }
            let Ok(entry) = entry else {
                truncated = true;
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let file_type = match entry.file_type() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if is_reparse_point(&entry, &file_type) {
                continue;
            }
            if file_type.is_file() && name == "package.json" {
                packages.push(entry.path());
                if packages.len() == MAX_PACKAGES_PER_ROOT {
                    truncated = true;
                    break;
                }
            } else if file_type.is_dir() && depth < MAX_DEPTH && !skipped_directory(name) {
                pending.push((entry.path(), depth + 1));
            }
        }
        if entry_count == MAX_DIRECTORIES_PER_ROOT {
            truncated = true;
        }
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
    let metadata = fs::metadata(path)
        .map_err(|_| PreviewError::internal("Preview package metadata could not be read."))?;
    if metadata.len() > MAX_PACKAGE_BYTES {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path)
        .map_err(|_| PreviewError::internal("Preview package manifest could not be read."))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| PreviewError::invalid("Preview package manifest is invalid."))?;
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
            .is_some_and(|value| !value.trim().is_empty() && value.len() <= 4096)
        {
            result.push(script);
        }
    }
    Ok(result)
}
