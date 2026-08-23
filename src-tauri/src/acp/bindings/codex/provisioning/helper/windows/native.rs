use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Foundation::{GENERIC_WRITE, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, READ_CONTROL, VOLUME_NAME_DOS, WRITE_DAC, WRITE_OWNER,
};

use super::super::ElevatedHelperError;
use super::super::transaction::NodeIdentity;
use crate::acp::bindings::codex::physical::open_exact_state_root;

const NEXT_SUFFIX: &str = ".next";
const CREATING_SUFFIX: &str = ".creating";
const HANDLE_PATH_WIDE_MAX: usize = 32 * 1024;
// FILE_INFORMATION_CLASS::FileRenameInformation from the Windows native API.
const FILE_RENAME_INFORMATION_CLASS: i32 = 10;
const STATUS_OBJECT_NAME_COLLISION: i32 = 0xc000_0035_u32 as i32;

#[repr(C)]
union IoStatus {
    status: i32,
    _pointer: *mut std::ffi::c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: IoStatus,
    information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetInformationFile(
        file_handle: HANDLE,
        io_status_block: *mut IoStatusBlock,
        file_information: *mut std::ffi::c_void,
        length: u32,
        file_information_class: i32,
    ) -> i32;
}

pub(super) fn node_identity(file: &File) -> Result<NodeIdentity, ElevatedHelperError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` remains live and `info` is writable for the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ElevatedHelperError::Drift);
    }
    Ok(NodeIdentity {
        volume_serial: info.dwVolumeSerialNumber,
        file_id: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

pub(super) fn handle_path(file: &File) -> Result<PathBuf, ElevatedHelperError> {
    let mut buffer = vec![0_u16; HANDLE_PATH_WIDE_MAX];
    // SAFETY: `buffer` is writable and `file` owns a live disk handle.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle() as HANDLE,
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| ElevatedHelperError::Drift)?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let length = usize::try_from(length).map_err(|_| ElevatedHelperError::Drift)?;
    if length == 0 || length >= buffer.len() {
        return Err(ElevatedHelperError::Drift);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

pub(super) fn open_mutation_directory(
    path: &Path,
    expected: Option<NodeIdentity>,
) -> Result<(File, NodeIdentity), ElevatedHelperError> {
    let mut options = OpenOptions::new();
    options
        .access_mode(DELETE | FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC | WRITE_OWNER)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let file = options.open(path).map_err(|_| ElevatedHelperError::Drift)?;
    let identity = node_identity(&file)?;
    if expected.is_some_and(|expected| expected != identity) {
        return Err(ElevatedHelperError::Drift);
    }
    Ok((file, identity))
}

pub(super) fn open_rename_directory(
    path: &Path,
    expected: NodeIdentity,
) -> Result<File, ElevatedHelperError> {
    let mut options = OpenOptions::new();
    options
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let file = options.open(path).map_err(|_| ElevatedHelperError::Drift)?;
    if node_identity(&file)? != expected {
        return Err(ElevatedHelperError::Drift);
    }
    Ok(file)
}

pub(super) fn open_destination_root(path: &Path) -> Result<File, ElevatedHelperError> {
    let (_, exact) = open_exact_state_root(path).map_err(|_| ElevatedHelperError::Drift)?;
    let expected = node_identity(&exact)?;
    let mut options = OpenOptions::new();
    options
        .access_mode(FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let root = options.open(path).map_err(|_| ElevatedHelperError::Drift)?;
    if node_identity(&root)? != expected {
        return Err(ElevatedHelperError::Drift);
    }
    Ok(root)
}

pub(super) fn rename_exact(
    source: &File,
    destination_root: &File,
    destination_name: &OsStr,
    replace: bool,
) -> Result<(), ElevatedHelperError> {
    let mut components = Path::new(destination_name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(ElevatedHelperError::HandoffIdentity);
    }
    let name = destination_name.encode_wide().collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or(ElevatedHelperError::Publication)?;
    let total = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(name_bytes as usize)
        .ok_or(ElevatedHelperError::Publication)?;
    let mut storage = vec![0_u64; total.div_ceil(std::mem::size_of::<u64>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: `storage` is aligned, zeroed, and sized for the fixed header plus
    // every UTF-16 code unit copied into its trailing filename buffer.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replace;
        (*info).RootDirectory = destination_root.as_raw_handle() as HANDLE;
        (*info).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    let mut status_block = IoStatusBlock {
        status: IoStatus { status: 0 },
        information: 0,
    };
    // SAFETY: both handles remain live; `info` and `status_block` are valid for
    // the duration of this synchronous native filesystem operation.
    let status = unsafe {
        NtSetInformationFile(
            source.as_raw_handle() as HANDLE,
            &mut status_block,
            info.cast(),
            u32::try_from(total).map_err(|_| ElevatedHelperError::Publication)?,
            FILE_RENAME_INFORMATION_CLASS,
        )
    };
    if status < 0 {
        return Err(if !replace && status == STATUS_OBJECT_NAME_COLLISION {
            ElevatedHelperError::Collision
        } else {
            ElevatedHelperError::Publication
        });
    }
    Ok(())
}

pub(super) fn create_initial_journal(
    path: &Path,
    bytes: &[u8],
    parent: &File,
    _creation_nonce: &str,
) -> Result<(), ElevatedHelperError> {
    let mut suffix = path.as_os_str().to_os_string();
    suffix.push(CREATING_SUFFIX);
    let temporary = PathBuf::from(suffix);
    remove_exact_file_if_present(&temporary)?;
    let mut file = create_synced_file(&temporary, bytes)?;
    rename_exact(
        &file,
        parent,
        path.file_name()
            .ok_or(ElevatedHelperError::HandoffIdentity)?,
        false,
    )?;
    file.flush()
        .and_then(|()| file.sync_all())
        .map_err(|_| ElevatedHelperError::Journal)
}

pub(super) fn cleanup_completed_stage(
    stage: &Path,
    authority: &mut Option<File>,
) -> Result<(), ElevatedHelperError> {
    let metadata = match fs::symlink_metadata(stage) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            authority.take();
            return Ok(());
        }
        Err(_) => return Err(ElevatedHelperError::Cleanup),
    };
    if !metadata.file_type().is_dir()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ElevatedHelperError::Cleanup);
    }
    let reopened = open_destination_root(stage).map_err(|_| ElevatedHelperError::Cleanup)?;
    if let Some(held) = authority.as_ref()
        && node_identity(held).map_err(|_| ElevatedHelperError::Cleanup)?
            != node_identity(&reopened).map_err(|_| ElevatedHelperError::Cleanup)?
    {
        return Err(ElevatedHelperError::Cleanup);
    }
    *authority = Some(reopened);
    let names = bounded_names(stage, 1, ElevatedHelperError::Cleanup)?;
    let handoff = OsString::from("handoff.json");
    if !names.is_empty() && names != BTreeSet::from([handoff]) {
        return Err(ElevatedHelperError::Cleanup);
    }
    if !names.is_empty() {
        remove_regular_file(&stage.join("handoff.json"), ElevatedHelperError::Cleanup)?;
        #[cfg(test)]
        run_cleanup_crash(CleanupCrashPoint::HandoffRemoved);
    }
    authority.take();
    fs::remove_dir(stage).map_err(|_| ElevatedHelperError::Cleanup)?;
    #[cfg(test)]
    run_cleanup_crash(CleanupCrashPoint::StageRemoved);
    Ok(())
}

pub(super) fn remove_exact_directory_tree(path: &Path) -> Result<(), ElevatedHelperError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ElevatedHelperError::Cleanup)?;
    if !metadata.file_type().is_dir()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ElevatedHelperError::Cleanup);
    }
    let mut entries = 0;
    remove_directory_contents(path, 0, &mut entries)?;
    fs::remove_dir(path).map_err(|_| ElevatedHelperError::Cleanup)
}

fn remove_directory_contents(
    path: &Path,
    depth: usize,
    entries: &mut usize,
) -> Result<(), ElevatedHelperError> {
    const DELETE_DEPTH_MAX: usize = 8;
    const DELETE_ENTRIES_MAX: usize = 32;
    if depth > DELETE_DEPTH_MAX {
        return Err(ElevatedHelperError::Cleanup);
    }
    let available = DELETE_ENTRIES_MAX
        .checked_sub(*entries)
        .ok_or(ElevatedHelperError::Cleanup)?;
    let children = bounded_directory_entries(
        fs::read_dir(path).map_err(|_| ElevatedHelperError::Cleanup)?,
        available,
        ElevatedHelperError::Cleanup,
    )?;
    *entries = entries
        .checked_add(children.len())
        .ok_or(ElevatedHelperError::Cleanup)?;
    for entry in children {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ElevatedHelperError::Cleanup)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ElevatedHelperError::Cleanup);
        }
        if metadata.file_type().is_dir() {
            remove_directory_contents(&path, depth + 1, entries)?;
            fs::remove_dir(path).map_err(|_| ElevatedHelperError::Cleanup)?;
        } else if metadata.file_type().is_file() {
            fs::remove_file(path).map_err(|_| ElevatedHelperError::Cleanup)?;
        } else {
            return Err(ElevatedHelperError::Cleanup);
        }
    }
    Ok(())
}

fn bounded_directory_entries<T, E>(
    entries: impl IntoIterator<Item = Result<T, E>>,
    maximum: usize,
    failure: ElevatedHelperError,
) -> Result<Vec<T>, ElevatedHelperError> {
    let mut bounded = Vec::with_capacity(maximum);
    for entry in entries {
        if bounded.len() == maximum {
            return Err(failure);
        }
        bounded.push(entry.map_err(|_| failure)?);
    }
    Ok(bounded)
}

#[cfg(test)]
mod platform_tests {
    use super::super::WindowsPlatform;
    use crate::acp::bindings::codex::provisioning::helper::transaction::HelperTransactionPlatform;

    #[test]
    fn windows_platform_owns_update_and_remove_transaction_primitives() {
        fn requires_concrete_lifecycle_platform<P: HelperTransactionPlatform>() {}

        requires_concrete_lifecycle_platform::<WindowsPlatform>();
        let _ = <WindowsPlatform as HelperTransactionPlatform>::hold_published_pair;
        let _ = <WindowsPlatform as HelperTransactionPlatform>::restore_held_pair;
        let _ = <WindowsPlatform as HelperTransactionPlatform>::retire_published_package;
        let _ = <WindowsPlatform as HelperTransactionPlatform>::retire_published_state;
    }
}

pub(super) fn replace_journal(
    path: &Path,
    bytes: &[u8],
    parent: &File,
) -> Result<(), ElevatedHelperError> {
    let mut suffix = path.as_os_str().to_os_string();
    suffix.push(NEXT_SUFFIX);
    let temporary = PathBuf::from(suffix);
    remove_exact_file_if_present(&temporary)?;
    let file = create_synced_file(&temporary, bytes)?;
    rename_exact(
        &file,
        parent,
        path.file_name()
            .ok_or(ElevatedHelperError::HandoffIdentity)?,
        true,
    )
    .and_then(|()| file.sync_all().map_err(|_| ElevatedHelperError::Journal))
    .map_err(|_| ElevatedHelperError::Journal)
}

fn create_synced_file(path: &Path, bytes: &[u8]) -> Result<File, ElevatedHelperError> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .access_mode(GENERIC_WRITE | DELETE)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let mut file = options
        .open(path)
        .map_err(|_| ElevatedHelperError::Journal)?;
    #[cfg(test)]
    if let Some(prefix) = TORN_INITIAL_WRITE.with(|slot| slot.replace(None)) {
        file.write_all(&bytes[..prefix.min(bytes.len())])
            .and_then(|()| file.sync_all())
            .map_err(|_| ElevatedHelperError::Journal)?;
        panic!("simulated crash during initial journal write");
    }
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| ElevatedHelperError::Journal)?;
    Ok(file)
}

fn remove_exact_file_if_present(path: &Path) -> Result<(), ElevatedHelperError> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_file()
                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 =>
        {
            fs::remove_file(path).map_err(|_| ElevatedHelperError::Journal)
        }
        Ok(_) => Err(ElevatedHelperError::Journal),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ElevatedHelperError::Journal),
    }
}

fn remove_regular_file(
    path: &Path,
    failure: ElevatedHelperError,
) -> Result<(), ElevatedHelperError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| failure)?;
    if !metadata.file_type().is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(failure);
    }
    fs::remove_file(path).map_err(|_| failure)
}

pub(super) fn bounded_names(
    root: &Path,
    maximum: usize,
    failure: ElevatedHelperError,
) -> Result<BTreeSet<OsString>, ElevatedHelperError> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(|_| failure)? {
        let name = entry.map_err(|_| failure)?.file_name();
        if !names.insert(name) || names.len() > maximum {
            return Err(failure);
        }
    }
    Ok(names)
}

#[cfg(test)]
thread_local! {
    static TORN_INITIAL_WRITE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static CLEANUP_CRASH: std::cell::Cell<Option<CleanupCrashPoint>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CleanupCrashPoint {
    HandoffRemoved,
    StageRemoved,
}

#[cfg(test)]
pub(super) fn inject_torn_initial_write(prefix: usize) {
    TORN_INITIAL_WRITE.with(|slot| {
        assert!(slot.replace(Some(prefix)).is_none());
    });
}

#[cfg(test)]
pub(super) fn inject_cleanup_crash(point: CleanupCrashPoint) {
    CLEANUP_CRASH.with(|slot| assert!(slot.replace(Some(point)).is_none()));
}

#[cfg(test)]
fn run_cleanup_crash(point: CleanupCrashPoint) {
    CLEANUP_CRASH.with(|slot| {
        if slot.get() == Some(point) {
            slot.set(None);
            panic!("simulated crash after cleanup effect {point:?}");
        }
    });
}

#[cfg(test)]
#[path = "native/tests.rs"]
mod tests;
