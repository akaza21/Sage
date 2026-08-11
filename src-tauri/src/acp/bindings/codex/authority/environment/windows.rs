use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK,
    GetFileInformationByHandle, GetFileType, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;

use super::EnvironmentAuthorityError;

const WINDOWS_PATH_WIDE_MAX: usize = 32 * 1024;

#[derive(Debug)]
pub(super) struct WindowsDirectoryProof {
    _directory: File,
}

pub(super) fn inspect(
    supplied_system_root: &str,
    supplied_windir: &str,
) -> Result<(String, WindowsDirectoryProof), EnvironmentAuthorityError> {
    let windows_directory = os_windows_directory()?;
    let windows_key = path_key(&windows_directory);
    if path_key(Path::new(supplied_system_root)) != windows_key
        || path_key(Path::new(supplied_windir)) != windows_key
    {
        return Err(EnvironmentAuthorityError::SystemRoot);
    }

    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let directory = options
        .open(&windows_directory)
        .map_err(|_| EnvironmentAuthorityError::SystemRoot)?;
    let raw = directory.as_raw_handle() as HANDLE;
    if unsafe { GetFileType(raw) } != FILE_TYPE_DISK {
        return Err(EnvironmentAuthorityError::SystemRoot);
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || path_key(&final_path(&directory)?) != windows_key
    {
        return Err(EnvironmentAuthorityError::SystemRoot);
    }
    let value = windows_directory
        .to_str()
        .ok_or(EnvironmentAuthorityError::SystemRoot)?
        .to_owned();
    Ok((
        value,
        WindowsDirectoryProof {
            _directory: directory,
        },
    ))
}

fn os_windows_directory() -> Result<PathBuf, EnvironmentAuthorityError> {
    let mut buffer = vec![0_u16; WINDOWS_PATH_WIDE_MAX];
    let length = unsafe {
        GetWindowsDirectoryW(
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| EnvironmentAuthorityError::SystemRoot)?,
        )
    };
    let length = usize::try_from(length).map_err(|_| EnvironmentAuthorityError::SystemRoot)?;
    if length == 0 || length >= buffer.len() {
        return Err(EnvironmentAuthorityError::SystemRoot);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn final_path(file: &File) -> Result<PathBuf, EnvironmentAuthorityError> {
    let mut buffer = vec![0_u16; WINDOWS_PATH_WIDE_MAX];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle() as HANDLE,
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| EnvironmentAuthorityError::SystemRoot)?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let length = usize::try_from(length).map_err(|_| EnvironmentAuthorityError::SystemRoot)?;
    if length == 0 || length >= buffer.len() {
        return Err(EnvironmentAuthorityError::SystemRoot);
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
