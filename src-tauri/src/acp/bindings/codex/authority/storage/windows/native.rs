use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FILE_TYPE_DISK, FileBasicInfo,
    FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx, GetFileType,
    GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
};

use super::StateAuthorityError;

const HANDLE_PATH_WIDE_MAX: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NativeIdentity {
    pub(super) volume: u64,
    pub(super) file_id: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeInfo {
    pub(super) identity: NativeIdentity,
    pub(super) attributes: u32,
    pub(super) links: u32,
    pub(super) logical_bytes: u64,
    pub(super) allocated_bytes: u64,
    pub(super) change_time: i64,
}

pub(super) fn open_node(
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

pub(super) fn native_info(file: &File) -> Result<NativeInfo, StateAuthorityError> {
    let raw = file.as_raw_handle() as HANDLE;
    let mut identity = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            raw,
            FileIdInfo,
            (&raw mut identity).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>())
                .map_err(|_| StateAuthorityError::Io)?,
        )
    } == 0
    {
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
            volume: identity.VolumeSerialNumber,
            file_id: identity.FileId.Identifier,
        },
        attributes: basic.FileAttributes,
        links: standard.NumberOfLinks,
        logical_bytes: u64::try_from(standard.EndOfFile).map_err(|_| StateAuthorityError::Io)?,
        allocated_bytes: u64::try_from(standard.AllocationSize)
            .map_err(|_| StateAuthorityError::Io)?,
        change_time: basic.ChangeTime,
    })
}

pub(super) fn final_path(file: &File) -> Result<PathBuf, StateAuthorityError> {
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

pub(super) fn path_key(path: &Path) -> Vec<u16> {
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

fn reject_reparse(attributes: u32) -> Result<(), StateAuthorityError> {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Err(StateAuthorityError::Alias)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_identity_preserves_the_full_128_bit_file_id() {
        let lower = NativeIdentity {
            volume: 7_u64,
            file_id: [0_u8; 16],
        };
        let mut upper = lower;
        upper.file_id[15] = 1;
        assert_ne!(lower, upper);
    }

    #[test]
    fn reparse_attribute_is_rejected_before_traversal() {
        assert_eq!(
            reject_reparse(FILE_ATTRIBUTE_REPARSE_POINT),
            Err(StateAuthorityError::Alias)
        );
        assert_eq!(reject_reparse(0), Ok(()));
    }
}
