//! Which file an open handle reads, and which version of its contents: the
//! volume and file id name a file whatever it is called, and the length and
//! last write time tell a rewrite in place apart.

use std::{
    fs::{File, Metadata},
    io,
    time::SystemTime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileIdentity {
    volume: u64,
    file_id: u128,
    len: u64,
    modified: SystemTime,
}

impl FileIdentity {
    pub(super) fn of(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        let (volume, file_id) = volume_and_file_id(file, &metadata)?;
        Ok(Self {
            volume,
            file_id,
            len: metadata.len(),
            modified: metadata.modified()?,
        })
    }
}

#[cfg(unix)]
fn volume_and_file_id(_file: &File, metadata: &Metadata) -> io::Result<(u64, u128)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino().into()))
}

#[cfg(windows)]
fn volume_and_file_id(file: &File, _metadata: &Metadata) -> io::Result<(u64, u128)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx},
    };

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
