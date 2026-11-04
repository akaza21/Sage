//! Windows deletes through the handle that proved the identity, so the file
//! removed is exactly the file checked, whatever happens to its name, and
//! that handle keeps every writer out until the file is gone.

use std::{
    fs::{File, Metadata, OpenOptions},
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};

use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, HANDLE},
    Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        FileDispositionInfo, FileDispositionInfoEx, FileIdInfo, GetFileInformationByHandleEx,
        SetFileInformationByHandle,
    },
};

use super::super::VaultResult;

pub(super) fn volume_and_file_id(file: &File, _metadata: &Metadata) -> io::Result<(u64, u128)> {
    let mut information = FILE_ID_INFO::default();
    // SAFETY: the handle is live and the output is a FILE_ID_INFO of the size passed.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileIdInfo,
            (&raw mut information).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((
        information.VolumeSerialNumber,
        u128::from_le_bytes(information.FileId.Identifier),
    ))
}

pub(super) fn delete_if_same(
    path: &Path,
    verify: impl FnOnce(&File) -> VaultResult<()>,
) -> VaultResult<()> {
    // A link is opened as the link itself, so it fails the identity check.
    // Sharing only reading shuts writers out from the check to the delete:
    // the open fails while another program has the file open for writing,
    // and no program can open it for writing, rename or delete it until this
    // handle closes.
    let file = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    verify(&file)?;
    mark_for_deletion(&file)?;
    // The file is removed when this, the last handle Sage holds, closes.
    Ok(())
}

/// Removes the name at once where the volume supports POSIX deletion, and
/// otherwise marks the file the classic way, as `std::fs::remove_file` does.
fn mark_for_deletion(file: &File) -> io::Result<()> {
    let posix = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    match set_information(file, FileDispositionInfoEx, &posix) {
        Err(error) if posix_deletion_unsupported(&error) => set_information(
            file,
            FileDispositionInfo,
            &FILE_DISPOSITION_INFO { DeleteFile: true },
        ),
        result => result,
    }
}

/// The errors a volume or Windows version without POSIX deletion reports.
fn posix_deletion_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION)
    )
}

fn set_information<T>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS, value: &T) -> io::Result<()> {
    // SAFETY: the handle is live and `value` is the structure `class` names,
    // passed with its own size.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            class,
            (value as *const T).cast(),
            size_of::<T>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
