use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf, Prefix};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_NAME_NORMALIZED, FILE_SHARE_READ, FILE_TYPE_DISK, FileBasicInfo,
    GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
    GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
};

use super::super::{ExpectedPhysicalFile, PACKAGE_DEPTH_MAX, PhysicalInspectionError};

const HANDLE_PATH_WIDE_MAX: usize = 32 * 1024;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeInfo {
    volume: u32,
    index: u64,
    pub(super) attributes: u32,
    pub(super) links: u32,
    pub(super) bytes: u64,
    change_time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PathKey(Vec<u16>);

pub(super) fn open_root(
    path: &Path,
) -> Result<(PathBuf, File, NativeInfo), PhysicalInspectionError> {
    if !local_absolute_normalized(path) {
        return Err(PhysicalInspectionError::ConfiguredRoot);
    }
    let canonical = fs::canonicalize(path).map_err(|_| PhysicalInspectionError::ConfiguredRoot)?;
    if path_key(path) != path_key(&canonical) {
        return Err(PhysicalInspectionError::Alias);
    }
    let (handle, info, final_path) = open_node(&canonical)?;
    if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || path_key(&canonical) != path_key(&final_path)
    {
        return Err(PhysicalInspectionError::ConfiguredRoot);
    }
    Ok((canonical, handle, info))
}

pub(super) fn open_node(
    path: &Path,
) -> Result<(File, NativeInfo, PathBuf), PhysicalInspectionError> {
    open_node_with_share(path, FILE_SHARE_READ)
}

pub(super) fn open_mutable_directory(
    path: &Path,
) -> Result<(File, NativeInfo, PathBuf), PhysicalInspectionError> {
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;

    open_node_with_share(path, FILE_SHARE_READ | FILE_SHARE_WRITE)
}

fn open_node_with_share(
    path: &Path,
    share_mode: u32,
) -> Result<(File, NativeInfo, PathBuf), PhysicalInspectionError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(share_mode)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let file = options
        .open(path)
        .map_err(|_| PhysicalInspectionError::Io)?;
    let raw = file.as_raw_handle() as HANDLE;
    // SAFETY: raw is borrowed from a live File.
    if unsafe { GetFileType(raw) } != FILE_TYPE_DISK {
        return Err(PhysicalInspectionError::Identity);
    }
    let info = native_info(raw)?;
    reject_reparse_attributes(info.attributes)?;
    Ok((file, info, final_path(raw)?))
}

fn reject_reparse_attributes(attributes: u32) -> Result<(), PhysicalInspectionError> {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(PhysicalInspectionError::Alias);
    }
    Ok(())
}

pub(super) fn verify_file(
    path: &Path,
    file: &mut File,
    before: NativeInfo,
    expected: &ExpectedPhysicalFile,
) -> Result<(), PhysicalInspectionError> {
    if before.links != 1 {
        return Err(PhysicalInspectionError::Alias);
    }
    if before.bytes != expected.byte_len {
        return Err(PhysicalInspectionError::Identity);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| PhysicalInspectionError::Io)?;
    let mut reader = file.take(expected.byte_len.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut observed = 0_u64;
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| PhysicalInspectionError::Io)?;
        if read == 0 {
            break;
        }
        observed = observed
            .checked_add(read as u64)
            .ok_or(PhysicalInspectionError::Identity)?;
        hasher.update(&buffer[..read]);
    }
    #[cfg(test)]
    run_after_hash(path);
    if observed != expected.byte_len {
        return Err(PhysicalInspectionError::ChangedDuringInspection);
    }
    verify_unchanged(path, file, before)?;
    let digest: [u8; 32] = hasher.finalize().into();
    if digest != expected.sha256 {
        return Err(PhysicalInspectionError::Digest);
    }
    Ok(())
}

pub(super) fn verify_unchanged(
    path: &Path,
    file: &File,
    before: NativeInfo,
) -> Result<(), PhysicalInspectionError> {
    let raw = file.as_raw_handle() as HANDLE;
    if before != native_info(raw)? || path_key(path) != path_key(&final_path(raw)?) {
        return Err(PhysicalInspectionError::ChangedDuringInspection);
    }
    Ok(())
}

fn native_info(handle: HANDLE) -> Result<NativeInfo, PhysicalInspectionError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: info is writable and handle is live for the call.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(PhysicalInspectionError::Io);
    }
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: basic is writable and handle is live for the call.
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&raw mut basic).cast(),
            u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>())
                .map_err(|_| PhysicalInspectionError::Identity)?,
        )
    } == 0
    {
        return Err(PhysicalInspectionError::Io);
    }
    let result = NativeInfo {
        volume: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        attributes: info.dwFileAttributes,
        links: info.nNumberOfLinks,
        bytes: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        change_time: basic.ChangeTime,
    };
    #[cfg(test)]
    let result = FORCE_NEXT_INFO_CHANGE.with(|forced| {
        if forced.replace(false) {
            NativeInfo {
                bytes: result.bytes.saturating_add(1),
                ..result
            }
        } else {
            result
        }
    });
    Ok(result)
}

fn final_path(handle: HANDLE) -> Result<PathBuf, PhysicalInspectionError> {
    let mut buffer = vec![0_u16; HANDLE_PATH_WIDE_MAX];
    // SAFETY: buffer is writable and handle is a live disk node.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| PhysicalInspectionError::Identity)?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let length = usize::try_from(length).map_err(|_| PhysicalInspectionError::Identity)?;
    if length == 0 || length >= buffer.len() {
        return Err(PhysicalInspectionError::Identity);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

pub(super) fn require_disjoint_state(
    path: &Path,
    package_root: &Path,
) -> Result<(), PhysicalInspectionError> {
    let normalized = normalize_without_creating(path)?;
    let home = path_key(&normalized);
    let package = path_key(package_root);
    if same_or_descendant(&home, &package) || same_or_descendant(&package, &home) {
        return Err(PhysicalInspectionError::StatePath);
    }
    Ok(())
}

fn normalize_without_creating(path: &Path) -> Result<PathBuf, PhysicalInspectionError> {
    if !local_absolute_normalized(path) {
        return Err(PhysicalInspectionError::StatePath);
    }
    let mut components = path.components();
    let mut probe = PathBuf::new();
    probe.push(
        components
            .next()
            .ok_or(PhysicalInspectionError::StatePath)?
            .as_os_str(),
    );
    probe.push(
        components
            .next()
            .ok_or(PhysicalInspectionError::StatePath)?
            .as_os_str(),
    );
    let mut normalized = inspect_existing_state_directory(&probe)?;
    let mut missing = false;
    for component in components {
        probe.push(component.as_os_str());
        if missing {
            normalized.push(component.as_os_str());
            continue;
        }
        match fs::symlink_metadata(&probe) {
            Ok(_) => normalized = inspect_existing_state_directory(&probe)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                normalized.push(component.as_os_str());
            }
            Err(_) => return Err(PhysicalInspectionError::StatePath),
        }
    }
    if path_key(path) != path_key(&normalized) {
        return Err(PhysicalInspectionError::StatePath);
    }
    Ok(normalized)
}

fn inspect_existing_state_directory(path: &Path) -> Result<PathBuf, PhysicalInspectionError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PhysicalInspectionError::StatePath)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(PhysicalInspectionError::StatePath);
    }
    let canonical = fs::canonicalize(path).map_err(|_| PhysicalInspectionError::StatePath)?;
    if path_key(path) != path_key(&canonical) {
        return Err(PhysicalInspectionError::StatePath);
    }
    Ok(canonical)
}

pub(super) fn validate_relative(path: &Path) -> Result<(), PhysicalInspectionError> {
    let depth = path.components().count();
    if path.as_os_str().is_empty()
        || depth == 0
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(PhysicalInspectionError::Identity);
    }
    if depth > PACKAGE_DEPTH_MAX {
        return Err(PhysicalInspectionError::DepthLimit);
    }
    Ok(())
}

pub(super) fn local_absolute_normalized(path: &Path) -> bool {
    let mut components = path.components();
    let local = matches!(
        components.next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
    );
    local
        && matches!(components.next(), Some(Component::RootDir))
        && components.all(|part| matches!(part, Component::Normal(_)))
}

pub(super) fn path_key(path: &Path) -> PathKey {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == b'/' as u16 {
                b'\\' as u16
            } else {
                unit
            }
        })
        .collect();
    let start = if wide.starts_with(&[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]) {
        4
    } else {
        0
    };
    let mut value = wide[start..].to_vec();
    while value.last() == Some(&(b'\\' as u16)) {
        value.pop();
    }
    for unit in &mut value {
        if (*unit >= b'A' as u16) && (*unit <= b'Z' as u16) {
            *unit += u16::from(b'a' - b'A');
        }
    }
    PathKey(value)
}

fn same_or_descendant(path: &PathKey, parent: &PathKey) -> bool {
    path == parent
        || (path.0.starts_with(&parent.0)
            && path.0.get(parent.0.len()).copied() == Some(b'\\' as u16))
}

#[cfg(test)]
struct AfterHash {
    target: PathBuf,
    action: Box<dyn FnOnce(&Path)>,
}

#[cfg(test)]
thread_local! {
    static AFTER_HASH: std::cell::RefCell<Option<AfterHash>> = const { std::cell::RefCell::new(None) };
    static FORCE_NEXT_INFO_CHANGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn run_after_hash(path: &Path) {
    AFTER_HASH.with(|slot| {
        let matches = slot
            .borrow()
            .as_ref()
            .is_some_and(|hook| path_key(&hook.target) == path_key(path));
        if matches && let Some(hook) = slot.borrow_mut().take() {
            (hook.action)(path);
        }
    });
}

#[cfg(test)]
pub(super) fn inject_after_hash<F>(target: PathBuf, hook: F)
where
    F: FnOnce(&Path) + 'static,
{
    AFTER_HASH.with(|slot| {
        assert!(slot.borrow().is_none(), "after-hash test hook already set");
        *slot.borrow_mut() = Some(AfterHash {
            target,
            action: Box::new(hook),
        });
    });
}

#[cfg(test)]
pub(super) fn force_next_info_change() {
    FORCE_NEXT_INFO_CHANGE.with(|forced| forced.set(true));
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use super::*;

    #[test]
    fn native_path_keys_do_not_collapse_distinct_non_unicode_names() {
        let first = PathBuf::from(OsString::from_wide(&[
            b'C' as u16,
            b':' as u16,
            b'\\' as u16,
            0xd800,
        ]));
        let second = PathBuf::from(OsString::from_wide(&[
            b'C' as u16,
            b':' as u16,
            b'\\' as u16,
            0xd801,
        ]));

        assert_ne!(path_key(&first), path_key(&second));
    }

    #[test]
    fn reparse_attribute_is_always_rejected() {
        assert_eq!(
            reject_reparse_attributes(FILE_ATTRIBUTE_REPARSE_POINT),
            Err(PhysicalInspectionError::Alias)
        );
    }
}
