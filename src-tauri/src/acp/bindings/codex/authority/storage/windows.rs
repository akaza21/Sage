use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO,
    FILE_TYPE_DISK, FileBasicInfo, FileStandardInfo, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetFileType, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
};

use super::{StateAuthorityError, StorageTotals, validate_totals};
use crate::acp::bindings::codex::{
    authority::{VerifiedCodexConfig, verify_codex_config},
    manifest::MetadataValidatedPacket,
    policy::{
        STATE_NATIVE_PATH_BYTES_MAX, STATE_RELATIVE_PATH_BYTES_MAX, SecurityPolicyPrerequisite,
    },
};

const HANDLE_PATH_WIDE_MAX: usize = 32 * 1024;
const CONFIG_READ_BYTES_MAX: u64 = 4 * 1024 + 1;

#[path = "windows/streams.rs"]
mod streams;
use streams::{reject_directory_named_streams, reject_named_streams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct NativeIdentity {
    volume: u32,
    index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeInfo {
    identity: NativeIdentity,
    attributes: u32,
    links: u32,
    logical_bytes: u64,
    allocated_bytes: u64,
    change_time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StateSnapshot {
    totals: StorageTotals,
    nodes: BTreeMap<PathBuf, NativeInfo>,
}

#[derive(Debug)]
pub(super) struct WindowsStateProof {
    home: PathBuf,
    root: File,
    root_info: NativeInfo,
    config_path: PathBuf,
    config: File,
    config_info: NativeInfo,
    snapshot: StateSnapshot,
}

impl WindowsStateProof {
    pub(super) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
        policy: &SecurityPolicyPrerequisite,
        config: &VerifiedCodexConfig,
    ) -> Result<(), StateAuthorityError> {
        if path_key(packet.codex_home()) != path_key(&self.home)
            || path_key(&final_path(&self.root)?) != path_key(&self.home)
            || native_info(&self.root)? != self.root_info
            || path_key(&final_path(&self.config)?) != path_key(&self.config_path)
            || native_info(&self.config)? != self.config_info
        {
            return Err(StateAuthorityError::ChangedDuringInspection);
        }
        verify_config_handle(&self.config, policy, config)?;
        let first = observe(&self.home)?;
        let second = observe(&self.home)?;
        if first != second || first != self.snapshot {
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
        || native_info(&root)? != root_info
        || native_info(&config_handle)? != config_info
    {
        return Err(StateAuthorityError::ChangedDuringInspection);
    }
    Ok(WindowsStateProof {
        home,
        root,
        root_info,
        config_path,
        config: config_handle,
        config_info,
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
    Ok(StateSnapshot { totals, nodes })
}

fn open_node(
    path: &Path,
    lock_config: bool,
) -> Result<(File, NativeInfo, PathBuf), StateAuthorityError> {
    let mut options = OpenOptions::new();
    let share = if lock_config {
        FILE_SHARE_READ
    } else {
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
    };
    options
        .read(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let file = options.open(path).map_err(|_| StateAuthorityError::Io)?;
    if unsafe { GetFileType(file.as_raw_handle() as HANDLE) } != FILE_TYPE_DISK {
        return Err(StateAuthorityError::Alias);
    }
    let info = native_info(&file)?;
    reject_reparse(info.attributes)?;
    let final_path = final_path(&file)?;
    Ok((file, info, final_path))
}

fn native_info(file: &File) -> Result<NativeInfo, StateAuthorityError> {
    let raw = file.as_raw_handle() as HANDLE;
    let mut identity = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(raw, &mut identity) } == 0 {
        return Err(StateAuthorityError::Io);
    }
    let mut standard = FILE_STANDARD_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            raw,
            FileStandardInfo,
            (&raw mut standard).cast(),
            u32::try_from(std::mem::size_of::<FILE_STANDARD_INFO>())
                .map_err(|_| StateAuthorityError::Io)?,
        )
    } == 0
    {
        return Err(StateAuthorityError::Io);
    }
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            raw,
            FileBasicInfo,
            (&raw mut basic).cast(),
            u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>())
                .map_err(|_| StateAuthorityError::Io)?,
        )
    } == 0
    {
        return Err(StateAuthorityError::Io);
    }
    Ok(NativeInfo {
        identity: NativeIdentity {
            volume: identity.dwVolumeSerialNumber,
            index: (u64::from(identity.nFileIndexHigh) << 32) | u64::from(identity.nFileIndexLow),
        },
        attributes: identity.dwFileAttributes,
        links: standard.NumberOfLinks,
        logical_bytes: u64::try_from(standard.EndOfFile).map_err(|_| StateAuthorityError::Io)?,
        allocated_bytes: u64::try_from(standard.AllocationSize)
            .map_err(|_| StateAuthorityError::Io)?,
        change_time: basic.ChangeTime,
    })
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

fn final_path(file: &File) -> Result<PathBuf, StateAuthorityError> {
    let mut buffer = vec![0_u16; HANDLE_PATH_WIDE_MAX];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle() as HANDLE,
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| StateAuthorityError::Io)?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let length = usize::try_from(length).map_err(|_| StateAuthorityError::Io)?;
    if length == 0 || length >= buffer.len() {
        return Err(StateAuthorityError::Io);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn path_key(path: &Path) -> Vec<u16> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.starts_with(&[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]) {
        wide.drain(..4);
    }
    while wide.last() == Some(&(b'\\' as u16)) {
        wide.pop();
    }
    for unit in &mut wide {
        if (*unit >= b'A' as u16) && (*unit <= b'Z' as u16) {
            *unit += u16::from(b'a' - b'A');
        }
    }
    wide
}

fn relative_key(path: &Path) -> String {
    path.as_os_str().to_string_lossy().to_ascii_lowercase()
}

fn reject_reparse(attributes: u32) -> Result<(), StateAuthorityError> {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Err(StateAuthorityError::Alias)
    } else {
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static BEFORE_STATE_RECHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
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
    fn reparse_attribute_is_rejected_before_traversal() {
        assert_eq!(
            reject_reparse(FILE_ATTRIBUTE_REPARSE_POINT),
            Err(StateAuthorityError::Alias)
        );
        assert_eq!(reject_reparse(0), Ok(()));
    }
}
