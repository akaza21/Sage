use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

#[path = "windows/handle.rs"]
mod handle;
use handle::{
    local_absolute_normalized, open_node, open_root, path_key, relative_key,
    require_disjoint_state, validate_relative, verify_file,
};

use super::{
    ConfiguredManagedPackageRoot, EXPECTED_FILE_COUNT, ExpectedPhysicalFile, InspectionClaims,
    PACKAGE_DEPTH_MAX, PACKAGE_ENTRY_MAX, PhysicalInspectionError, PhysicallyInspectedPackage,
    decode_digest,
};
use crate::acp::bindings::codex::manifest::MetadataValidatedPacket;

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
    let (package_root, root_handle) = open_root(&claims.package_root)?;
    require_disjoint_state(&claims.codex_home, &package_root)?;

    let mut expected_files = BTreeMap::new();
    let mut expected_directories = BTreeSet::new();
    for expected in claims.files {
        validate_relative(&expected.relative_path)?;
        let key = relative_key(&expected.relative_path);
        if expected_files.insert(key, expected).is_some() {
            return Err(PhysicalInspectionError::Closure);
        }
    }
    for expected in expected_files.values() {
        let mut parent = expected.relative_path.parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            expected_directories.insert(relative_key(path));
            parent = path.parent();
        }
    }

    let mut held_handles = vec![root_handle];
    let mut pending = vec![PathBuf::new()];
    let mut entry_count = 0_usize;
    let mut artifact_count = 0_usize;
    let mut aggregate_bytes = 0_u64;
    let mut unexpected = false;
    while let Some(relative_directory) = pending.pop() {
        let directory = package_root.join(&relative_directory);
        for entry in fs::read_dir(&directory).map_err(|_| PhysicalInspectionError::Io)? {
            let entry = entry.map_err(|_| PhysicalInspectionError::Io)?;
            entry_count = entry_count
                .checked_add(1)
                .ok_or(PhysicalInspectionError::EntryLimit)?;
            if entry_count > PACKAGE_ENTRY_MAX {
                return Err(PhysicalInspectionError::EntryLimit);
            }
            let relative = relative_directory.join(entry.file_name());
            let depth = relative.components().count();
            if depth > PACKAGE_DEPTH_MAX {
                return Err(PhysicalInspectionError::DepthLimit);
            }
            let path = package_root.join(&relative);
            let (mut handle, before, final_path) = open_node(&path)?;
            if path_key(&path) != path_key(&final_path) {
                return Err(PhysicalInspectionError::Alias);
            }
            let key = relative_key(&relative);
            if before.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                if !expected_directories.remove(&key) {
                    unexpected = true;
                }
                pending.push(relative);
            } else if let Some(expected) = expected_files.remove(&key) {
                verify_file(&path, &mut handle, before, &expected)?;
                aggregate_bytes = aggregate_bytes
                    .checked_add(before.bytes)
                    .ok_or(PhysicalInspectionError::Identity)?;
                artifact_count += 1;
            } else {
                if before.links != 1 {
                    return Err(PhysicalInspectionError::Alias);
                }
                unexpected = true;
            }
            held_handles.push(handle);
        }
    }
    if unexpected
        || !expected_files.is_empty()
        || !expected_directories.is_empty()
        || artifact_count != EXPECTED_FILE_COUNT
        || aggregate_bytes != claims.aggregate_bytes
    {
        return Err(PhysicalInspectionError::Closure);
    }
    Ok(PhysicallyInspectedPackage {
        package_root,
        artifact_count,
        byte_len: aggregate_bytes,
        _held_handles: held_handles,
    })
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
