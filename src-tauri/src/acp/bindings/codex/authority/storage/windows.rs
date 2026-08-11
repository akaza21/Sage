use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

use super::{StateAuthorityError, StorageTotals, validate_totals};
use crate::acp::bindings::codex::{
    authority::{VerifiedCodexConfig, verify_codex_config},
    manifest::MetadataValidatedPacket,
    policy::{
        STATE_NATIVE_PATH_BYTES_MAX, STATE_RELATIVE_PATH_BYTES_MAX, SecurityPolicyPrerequisite,
    },
};

const CONFIG_READ_BYTES_MAX: u64 = 4 * 1024 + 1;

#[path = "windows/native.rs"]
mod native;
#[path = "windows/streams.rs"]
mod streams;
use native::{NativeInfo, final_path, native_info, open_node, path_key};
use streams::{reject_directory_named_streams, reject_named_streams};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StateSnapshot {
    root: NativeInfo,
    config: NativeInfo,
    totals: StorageTotals,
    nodes: BTreeMap<PathBuf, NativeInfo>,
}

#[derive(Debug)]
pub(super) struct WindowsStateProof {
    home: PathBuf,
    root: File,
    config_path: PathBuf,
    config: File,
    snapshot: StateSnapshot,
}

impl WindowsStateProof {
    pub(super) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
        policy: &SecurityPolicyPrerequisite,
        config: &VerifiedCodexConfig,
    ) -> Result<(), StateAuthorityError> {
        if path_key(packet.codex_home()) != path_key(&self.home) {
            return Err(StateAuthorityError::ChangedDuringInspection);
        }
        verify_config_handle(&self.config, policy, config)?;
        let first = observe(&self.home)?;
        let second = observe(&self.home)?;
        if first != second
            || first != self.snapshot
            || !retained_handles_match_snapshot(
                &self.home,
                &self.root,
                &self.config_path,
                &self.config,
                &second,
            )?
        {
            return Err(StateAuthorityError::ChangedDuringInspection);
        }
        Ok(())
    }
}

pub(super) fn inspect(
    packet: &MetadataValidatedPacket,
    policy: &SecurityPolicyPrerequisite,
    config: &VerifiedCodexConfig,
) -> Result<WindowsStateProof, StateAuthorityError> {
    let home = packet.codex_home().to_owned();
    let (root, root_info, final_home) = open_node(&home, false)?;
    if root_info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || path_key(&home) != path_key(&final_home)
    {
        return Err(StateAuthorityError::Alias);
    }
    reject_directory_named_streams(&home)?;
    #[cfg(test)]
    run_after_state_root_open();
    let config_path = home.join("config.toml");
    let (config_handle, config_info, final_config) = open_node(&config_path, true)?;
    if config_info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0
        || config_info.links != 1
        || path_key(&config_path) != path_key(&final_config)
    {
        return Err(StateAuthorityError::Alias);
    }
    reject_named_streams(&config_handle)?;
    verify_config_handle(&config_handle, policy, config)?;
    let snapshot = observe(&home)?;
    #[cfg(test)]
    run_before_state_recheck();
    let reobserved = observe(&home)?;
    if snapshot != reobserved
        || !retained_handles_match_snapshot(
            &home,
            &root,
            &config_path,
            &config_handle,
            &reobserved,
        )?
    {
        return Err(StateAuthorityError::ChangedDuringInspection);
    }
    Ok(WindowsStateProof {
        home,
        root,
        config_path,
        config: config_handle,
        snapshot,
    })
}

fn observe(home: &Path) -> Result<StateSnapshot, StateAuthorityError> {
    let mut totals = StorageTotals {
        regular_files: 0,
        directories: 1,
        maximum_depth: 0,
        logical_bytes: 0,
        allocated_bytes: 0,
        maximum_file_bytes: 0,
        credential_file_bytes: 0,
    };
    let mut nodes = BTreeMap::new();
    let mut identities = BTreeSet::new();
    let mut observed_config = None;
    let (root, root_info, final_root) = open_node(home, false)?;
    if root_info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || path_key(home) != path_key(&final_root)
        || !identities.insert(root_info.identity)
    {
        return Err(StateAuthorityError::Alias);
    }
    reject_directory_named_streams(home)?;
    let mut pending = vec![(PathBuf::new(), home.to_owned(), root, root_info)];
    while let Some((relative_parent, directory, handle, before)) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|_| StateAuthorityError::Io)? {
            let entry = entry.map_err(|_| StateAuthorityError::Io)?;
            let relative = relative_parent.join(entry.file_name());
            validate_path(&relative, &home.join(&relative))?;
            let path = home.join(&relative);
            let (node, info, final_node) = open_node(&path, false)?;
            if path_key(&path) != path_key(&final_node) || !identities.insert(info.identity) {
                return Err(StateAuthorityError::Alias);
            }
            if relative_key(&relative) == "config.toml" {
                observed_config = Some(info);
            }
            let depth = u8::try_from(relative.components().count())
                .map_err(|_| StateAuthorityError::DepthLimit)?;
            totals.maximum_depth = totals.maximum_depth.max(depth);
            if info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                reject_directory_named_streams(&path)?;
                totals.directories = totals
                    .directories
                    .checked_add(1)
                    .ok_or(StateAuthorityError::DirectoryLimit)?;
                validate_totals(totals)?;
                pending.push((relative.clone(), path, node, info));
            } else {
                reject_named_streams(&node)?;
                if info.links != 1 {
                    return Err(StateAuthorityError::Alias);
                }
                totals.regular_files = totals
                    .regular_files
                    .checked_add(1)
                    .ok_or(StateAuthorityError::FileLimit)?;
                totals.logical_bytes = totals
                    .logical_bytes
                    .checked_add(info.logical_bytes)
                    .ok_or(StateAuthorityError::LogicalBytes)?;
                totals.allocated_bytes = totals
                    .allocated_bytes
                    .checked_add(info.allocated_bytes)
                    .ok_or(StateAuthorityError::AllocatedBytes)?;
                totals.maximum_file_bytes = totals.maximum_file_bytes.max(info.logical_bytes);
                if relative_key(&relative) == "auth.json" {
                    totals.credential_file_bytes = info.logical_bytes;
                }
                validate_totals(totals)?;
                if native_info(&node)? != info || path_key(&final_path(&node)?) != path_key(&path) {
                    return Err(StateAuthorityError::ChangedDuringInspection);
                }
            }
            if nodes.insert(relative, info).is_some() {
                return Err(StateAuthorityError::ChangedDuringInspection);
            }
        }
        if native_info(&handle)? != before
            || path_key(&final_path(&handle)?) != path_key(&directory)
        {
            return Err(StateAuthorityError::ChangedDuringInspection);
        }
    }
    validate_totals(totals)?;
    Ok(StateSnapshot {
        root: root_info,
        config: observed_config.ok_or(StateAuthorityError::Config)?,
        totals,
        nodes,
    })
}

fn retained_handles_match_snapshot(
    home: &Path,
    root: &File,
    config_path: &Path,
    config: &File,
    snapshot: &StateSnapshot,
) -> Result<bool, StateAuthorityError> {
    Ok(path_key(&final_path(root)?) == path_key(home)
        && path_key(&final_path(config)?) == path_key(config_path)
        && snapshot_matches_native_infos(snapshot, native_info(root)?, native_info(config)?))
}

fn snapshot_matches_native_infos(
    snapshot: &StateSnapshot,
    root: NativeInfo,
    config: NativeInfo,
) -> bool {
    snapshot.root == root && snapshot.config == config
}

fn verify_config_handle(
    config_file: &File,
    policy: &SecurityPolicyPrerequisite,
    expected: &VerifiedCodexConfig,
) -> Result<(), StateAuthorityError> {
    expected
        .revalidate(policy)
        .map_err(|_| StateAuthorityError::Config)?;
    let before = native_info(config_file)?;
    let mut reader = config_file
        .try_clone()
        .map_err(|_| StateAuthorityError::Io)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| StateAuthorityError::Io)?;
    let mut bytes = Vec::new();
    reader
        .take(CONFIG_READ_BYTES_MAX)
        .read_to_end(&mut bytes)
        .map_err(|_| StateAuthorityError::Io)?;
    verify_codex_config(&bytes, policy).map_err(|_| StateAuthorityError::Config)?;
    if native_info(config_file)? != before {
        return Err(StateAuthorityError::ChangedDuringInspection);
    }
    Ok(())
}

fn validate_path(relative: &Path, native: &Path) -> Result<(), StateAuthorityError> {
    let relative_bytes = relative.as_os_str().encode_wide().count().saturating_mul(2);
    let native_bytes = native.as_os_str().encode_wide().count().saturating_mul(2);
    if relative_bytes as u64 > STATE_RELATIVE_PATH_BYTES_MAX
        || native_bytes as u64 > STATE_NATIVE_PATH_BYTES_MAX
    {
        return Err(StateAuthorityError::PathLimit);
    }
    Ok(())
}

fn relative_key(path: &Path) -> String {
    path.as_os_str().to_string_lossy().to_ascii_lowercase()
}

#[cfg(test)]
thread_local! {
    static AFTER_STATE_ROOT_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
    static BEFORE_STATE_RECHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn inject_after_state_root_open<F>(hook: F)
where
    F: FnOnce() + 'static,
{
    AFTER_STATE_ROOT_OPEN.with(|slot| {
        assert!(slot.borrow().is_none(), "root-open hook already installed");
        *slot.borrow_mut() = Some(Box::new(hook));
    });
}

#[cfg(test)]
pub(crate) fn inject_before_state_recheck<F>(hook: F)
where
    F: FnOnce() + 'static,
{
    BEFORE_STATE_RECHECK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "state recheck hook already installed"
        );
        *slot.borrow_mut() = Some(Box::new(hook));
    });
}

#[cfg(test)]
fn run_after_state_root_open() {
    AFTER_STATE_ROOT_OPEN.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(test)]
fn run_before_state_recheck() {
    BEFORE_STATE_RECHECK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_authority_rejects_a_different_root_or_config_identity() {
        let info = NativeInfo {
            identity: native::NativeIdentity {
                volume: 7,
                file_id: [0; 16],
            },
            attributes: 0,
            links: 1,
            logical_bytes: 10,
            allocated_bytes: 16,
            change_time: 20,
        };
        let snapshot = StateSnapshot {
            root: info,
            config: info,
            totals: StorageTotals {
                regular_files: 0,
                directories: 1,
                maximum_depth: 0,
                logical_bytes: 0,
                allocated_bytes: 0,
                maximum_file_bytes: 0,
                credential_file_bytes: 0,
            },
            nodes: BTreeMap::new(),
        };
        let mut replacement = info;
        replacement.identity.file_id[15] = 1;
        assert!(snapshot_matches_native_infos(&snapshot, info, info));
        assert!(!snapshot_matches_native_infos(&snapshot, replacement, info));
        assert!(!snapshot_matches_native_infos(&snapshot, info, replacement));
    }
}
