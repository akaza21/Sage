use std::{fs, path::Path};

#[cfg(not(unix))]
use sage_core::error::AppError;
use sage_core::error::AppResult;

use super::RootIdentity;

#[cfg(unix)]
pub fn root_identity_path(path: &Path) -> AppResult<RootIdentity> {
    root_identity_metadata(&path.metadata()?)
}

#[cfg(unix)]
pub fn root_identity_metadata(metadata: &fs::Metadata) -> AppResult<RootIdentity> {
    use std::os::unix::fs::MetadataExt;

    let mut file_id = [0_u8; 16];
    file_id[8..].copy_from_slice(&metadata.ino().to_be_bytes());
    Ok(RootIdentity {
        volume: metadata.dev(),
        file_id,
    })
}

#[cfg(windows)]
pub fn root_identity_path(path: &Path) -> AppResult<RootIdentity> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    root_identity_file(&file)
}

#[cfg(windows)]
pub fn root_identity_file(file: &fs::File) -> AppResult<RootIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let mut information = FILE_ID_INFO::default();
    // SAFETY: the file handle is live and the typed output has its exact Windows size.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileIdInfo,
            (&raw mut information).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>()).map_err(|_| {
                AppError::InvalidRequest("Windows file identity size overflow".into())
            })?,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(RootIdentity {
        volume: information.VolumeSerialNumber,
        file_id: information.FileId.Identifier,
    })
}

#[cfg(not(any(unix, windows)))]
pub fn root_identity_path(_path: &Path) -> AppResult<RootIdentity> {
    Err(AppError::InvalidRequest(
        "safe file creation is unavailable on this platform".into(),
    ))
}

#[cfg(unix)]
pub fn leaf_identity(_file: &fs::File, metadata: &fs::Metadata) -> AppResult<RootIdentity> {
    root_identity_metadata(metadata)
}

#[cfg(windows)]
pub fn leaf_identity(file: &fs::File, _metadata: &fs::Metadata) -> AppResult<RootIdentity> {
    root_identity_file(file)
}

#[cfg(not(any(unix, windows)))]
pub fn leaf_identity(_file: &fs::File, _metadata: &fs::Metadata) -> AppResult<RootIdentity> {
    Err(AppError::InvalidRequest(
        "safe file identity is unavailable on this platform".into(),
    ))
}
