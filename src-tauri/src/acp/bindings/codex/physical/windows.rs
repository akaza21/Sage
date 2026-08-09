use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::PathBuf;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

#[path = "windows/handle.rs"]
mod handle;
use handle::{
    NativeInfo, local_absolute_normalized, open_node, open_root, path_key, require_disjoint_state,
    validate_relative, verify_file, verify_unchanged,
};

use super::{
    ConfiguredManagedPackageRoot, EXPECTED_FILE_COUNT, ExpectedPhysicalFile, InspectionClaims,
    PACKAGE_DEPTH_MAX, PACKAGE_ENTRY_MAX, PhysicalInspectionError, PhysicallyInspectedPackage,
    decode_digest,
};
use crate::acp::bindings::codex::manifest::MetadataValidatedPacket;

#[derive(Debug)]
struct HeldNode {
    relative_path: PathBuf,
    path: PathBuf,
    handle: File,
    initial: NativeInfo,
}

pub(super) fn inspect(
    packet: &MetadataValidatedPacket,
    configured_root: &ConfiguredManagedPackageRoot,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    let claims = claims_from_packet(packet, configured_root)?;
    inspect_claims(claims)
}

pub(super) fn claims_from_packet(
    packet: &MetadataValidatedPacket,
    configured_root: &ConfiguredManagedPackageRoot,
) -> Result<InspectionClaims, PhysicalInspectionError> {
    let root = configured_root.as_path();
    if !local_absolute_normalized(root) {
        return Err(PhysicalInspectionError::ConfiguredRoot);
    }
    let mut files = Vec::with_capacity(EXPECTED_FILE_COUNT);
    for (artifact, relative) in packet.physical_artifact_claims() {
        let relative_path = PathBuf::from(relative);
        if path_key(&artifact.path) != path_key(&root.join(&relative_path)) {
            return Err(PhysicalInspectionError::PacketRootMismatch);
        }
        files.push(ExpectedPhysicalFile {
            relative_path,
            sha256: decode_digest(&artifact.sha256)?,
            byte_len: artifact.byte_len,
        });
    }
    Ok(InspectionClaims {
        package_root: root.to_owned(),
        codex_home: packet.codex_home().to_owned(),
        files,
        aggregate_bytes: packet.metadata().package_unpacked_byte_len,
    })
}

pub(super) fn inspect_claims(
    claims: InspectionClaims,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    if claims.files.len() != EXPECTED_FILE_COUNT {
        return Err(PhysicalInspectionError::Closure);
    }
    let (package_root, root_handle, root_info) = open_root(&claims.package_root)?;
    require_disjoint_state(&claims.codex_home, &package_root)?;

    let mut expected_files = BTreeMap::new();
    let mut expected_directories = BTreeSet::new();
    for expected in claims.files {
        validate_relative(&expected.relative_path)?;
        let key = expected.relative_path.clone();
        if expected_files.insert(key, expected).is_some() {
            return Err(PhysicalInspectionError::Closure);
        }
    }
    for expected in expected_files.values() {
        let mut parent = expected.relative_path.parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            expected_directories.insert(path.to_owned());
            parent = path.parent();
        }
    }

    let mut held_directories = Vec::new();
    let mut held_files = Vec::new();
    let mut pending = vec![HeldNode {
        relative_path: PathBuf::new(),
        path: package_root.clone(),
        handle: root_handle,
        initial: root_info,
    }];
    let mut observed_namespace = BTreeSet::new();
    let mut entry_count = 0_usize;
    let mut artifact_count = 0_usize;
    let mut aggregate_bytes = 0_u64;
    let mut unexpected = false;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory.path).map_err(|_| PhysicalInspectionError::Io)? {
            let entry = entry.map_err(|_| PhysicalInspectionError::Io)?;
            entry_count = entry_count
                .checked_add(1)
                .ok_or(PhysicalInspectionError::EntryLimit)?;
            if entry_count > PACKAGE_ENTRY_MAX {
                return Err(PhysicalInspectionError::EntryLimit);
            }
            let relative = directory.relative_path.join(entry.file_name());
            let depth = relative.components().count();
            if depth > PACKAGE_DEPTH_MAX {
                return Err(PhysicalInspectionError::DepthLimit);
            }
            let path = package_root.join(&relative);
            let (mut handle, before, final_path) = open_node(&path)?;
            if path_key(&path) != path_key(&final_path) {
                return Err(PhysicalInspectionError::Alias);
            }
            if !observed_namespace.insert(relative.clone()) {
                return Err(PhysicalInspectionError::ChangedDuringInspection);
            }
            let key = relative.clone();
            if before.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                if !expected_directories.remove(&key) {
                    unexpected = true;
                }
                pending.push(HeldNode {
                    relative_path: relative,
                    path,
                    handle,
                    initial: before,
                });
            } else if let Some(expected) = expected_files.remove(&key) {
                verify_file(&path, &mut handle, before, &expected)?;
                aggregate_bytes = aggregate_bytes
                    .checked_add(before.bytes)
                    .ok_or(PhysicalInspectionError::Identity)?;
                artifact_count += 1;
                held_files.push(HeldNode {
                    relative_path: relative,
                    path,
                    handle,
                    initial: before,
                });
            } else {
                if before.links != 1 {
                    return Err(PhysicalInspectionError::Alias);
                }
                unexpected = true;
                held_files.push(HeldNode {
                    relative_path: relative,
                    path,
                    handle,
                    initial: before,
                });
            }
        }
        held_directories.push(directory);
    }
    if unexpected
        || !expected_files.is_empty()
        || !expected_directories.is_empty()
        || artifact_count != EXPECTED_FILE_COUNT
        || aggregate_bytes != claims.aggregate_bytes
    {
        return Err(PhysicalInspectionError::Closure);
    }
    #[cfg(test)]
    run_before_namespace_recheck();
    recheck_namespace(&held_directories, &held_files, &observed_namespace)?;
    let held_handles = held_directories
        .into_iter()
        .chain(held_files)
        .map(|node| node.handle)
        .collect();
    Ok(PhysicallyInspectedPackage {
        package_root,
        artifact_count,
        byte_len: aggregate_bytes,
        _held_handles: held_handles,
    })
}

fn recheck_namespace(
    directories: &[HeldNode],
    files: &[HeldNode],
    observed_namespace: &BTreeSet<PathBuf>,
) -> Result<(), PhysicalInspectionError> {
    let mut reobserved = BTreeSet::new();
    let mut entry_count = 0_usize;
    for directory in directories {
        verify_unchanged(&directory.path, &directory.handle, directory.initial)?;
        for entry in fs::read_dir(&directory.path).map_err(|_| PhysicalInspectionError::Io)? {
            let entry = entry.map_err(|_| PhysicalInspectionError::Io)?;
            entry_count = entry_count
                .checked_add(1)
                .ok_or(PhysicalInspectionError::EntryLimit)?;
            if entry_count > PACKAGE_ENTRY_MAX {
                return Err(PhysicalInspectionError::EntryLimit);
            }
            let relative = directory.relative_path.join(entry.file_name());
            if relative.components().count() > PACKAGE_DEPTH_MAX {
                return Err(PhysicalInspectionError::DepthLimit);
            }
            if !reobserved.insert(relative) {
                return Err(PhysicalInspectionError::ChangedDuringInspection);
            }
        }
    }
    if &reobserved != observed_namespace {
        return Err(PhysicalInspectionError::ChangedDuringInspection);
    }
    for node in directories.iter().chain(files) {
        verify_unchanged(&node.path, &node.handle, node.initial)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn inject_after_hash<F>(target: PathBuf, hook: F)
where
    F: FnOnce(&std::path::Path) + 'static,
{
    handle::inject_after_hash(target, hook);
}

#[cfg(test)]
pub(super) fn force_next_info_change() {
    handle::force_next_info_change();
}

#[cfg(test)]
thread_local! {
    static BEFORE_NAMESPACE_RECHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub(super) fn inject_before_namespace_recheck<F>(hook: F)
where
    F: FnOnce() + 'static,
{
    BEFORE_NAMESPACE_RECHECK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "namespace-recheck test hook already set"
        );
        *slot.borrow_mut() = Some(Box::new(hook));
    });
}

#[cfg(test)]
fn run_before_namespace_recheck() {
    BEFORE_NAMESPACE_RECHECK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}
