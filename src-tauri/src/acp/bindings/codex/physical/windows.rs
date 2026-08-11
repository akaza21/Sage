use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf, Prefix};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

#[path = "windows/handle.rs"]
mod handle;
use handle::{
    NativeInfo, local_absolute_normalized, open_node, open_root, path_key, require_disjoint_state,
    validate_relative, verify_file, verify_unchanged,
};

use super::{
    ConfiguredCodexSourceRoot, ConfiguredManagedPackageRoot, EXPECTED_FILE_COUNT,
    ExpectedPhysicalFile, HeldPhysicalNode, InspectionClaims, PACKAGE_DEPTH_MAX, PACKAGE_ENTRY_MAX,
    PhysicalInspectionError, PhysicallyInspectedPackage, decode_digest,
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

pub(super) fn inspect_source(
    packet: &MetadataValidatedPacket,
    configured_root: &ConfiguredCodexSourceRoot,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    inspect_claims(claims_from_source(packet, configured_root)?)
}

fn claims_from_source(
    packet: &MetadataValidatedPacket,
    configured_root: &ConfiguredCodexSourceRoot,
) -> Result<InspectionClaims, PhysicalInspectionError> {
    let root = configured_root.as_path();
    if !local_absolute_normalized(root) {
        return Err(PhysicalInspectionError::ConfiguredRoot);
    }
    require_disjoint_state(&packet.managed_package_root(), root)?;
    require_disjoint_state(packet.codex_home(), root)?;
    let files = packet
        .physical_artifact_claims()
        .map(|(artifact, relative)| {
            Ok(ExpectedPhysicalFile {
                relative_path: PathBuf::from(relative),
                sha256: decode_digest(&artifact.sha256)?,
                byte_len: artifact.byte_len,
            })
        })
        .collect::<Result<Vec<_>, PhysicalInspectionError>>()?;
    Ok(InspectionClaims {
        package_root: root.to_owned(),
        codex_home: packet.codex_home().to_owned(),
        files,
        aggregate_bytes: packet.metadata().package_unpacked_byte_len,
    })
}

pub(super) fn open_exact_state_root(
    path: &Path,
) -> Result<(PathBuf, File), PhysicalInspectionError> {
    let (handle, info, final_path) = handle::open_mutable_directory(path)?;
    if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 || path_key(path) != path_key(&final_path) {
        return Err(PhysicalInspectionError::StatePath);
    }
    Ok((final_path, handle))
}

pub(super) fn same_native_path(left: &Path, right: &Path) -> bool {
    handle::path_key(left) == handle::path_key(right)
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
        if !packet_path_matches_root_and_relative(&artifact.path, root, &relative_path) {
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

pub(super) fn proof_matches_packet(
    proof: &PhysicallyInspectedPackage,
    packet: &MetadataValidatedPacket,
) -> bool {
    let Ok(root) = ConfiguredManagedPackageRoot::new(proof.package_root.clone()) else {
        return false;
    };
    let Ok(claims) = claims_from_packet(packet, &root) else {
        return false;
    };
    claims.package_root == proof.package_root
        && claims.codex_home == proof.codex_home
        && claims.files.len() == proof.artifact_count
        && claims.aggregate_bytes == proof.byte_len
}

/// Matches exact packet-relative spelling while admitting only Windows root
/// representation aliases: separators, verbatim-disk prefix, and drive case.
fn packet_path_matches_root_and_relative(
    artifact_path: &Path,
    configured_root: &Path,
    pinned_relative: &Path,
) -> bool {
    let artifact_components: Vec<_> = artifact_path.components().collect();
    let relative_components: Vec<_> = pinned_relative.components().collect();
    let Some(root_len) = artifact_components
        .len()
        .checked_sub(relative_components.len())
    else {
        return false;
    };
    if root_len == 0
        || !artifact_components[root_len..]
            .iter()
            .zip(&relative_components)
            .all(|(declared, expected)| exact_normal_component(declared, expected))
    {
        return false;
    }
    let configured_components: Vec<_> = configured_root.components().collect();
    artifact_components[..root_len].len() == configured_components.len()
        && artifact_components[..root_len]
            .iter()
            .zip(&configured_components)
            .all(|(declared, configured)| matching_root_component(declared, configured))
}

fn exact_normal_component(declared: &Component<'_>, expected: &Component<'_>) -> bool {
    match (declared, expected) {
        (Component::Normal(declared), Component::Normal(expected)) => {
            declared.encode_wide().eq(expected.encode_wide())
        }
        _ => false,
    }
}

fn matching_root_component(declared: &Component<'_>, configured: &Component<'_>) -> bool {
    match (declared, configured) {
        (Component::Prefix(declared), Component::Prefix(configured)) => {
            let drive = |prefix: Prefix<'_>| match prefix {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(letter),
                _ => None,
            };
            drive(declared.kind())
                .zip(drive(configured.kind()))
                .is_some_and(|(declared, configured)| declared.eq_ignore_ascii_case(&configured))
        }
        (Component::RootDir, Component::RootDir) => true,
        _ => exact_normal_component(declared, configured),
    }
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
    let held_nodes = held_directories
        .into_iter()
        .chain(held_files)
        .map(|node| HeldPhysicalNode {
            path: node.path,
            directory: node.initial.attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            handle: node.handle,
        })
        .collect();
    Ok(PhysicallyInspectedPackage {
        package_root,
        codex_home: claims.codex_home,
        artifact_count,
        byte_len: aggregate_bytes,
        held_nodes,
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
