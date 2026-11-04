//! POSIX cannot unlink through a descriptor, so the folder is held open and
//! the name is checked and removed inside it. The checked file stays open
//! until the name is gone, so its inode cannot be freed and reused by a file
//! saved over the name in between.

use std::{
    ffi::{CString, OsStr},
    fs::{File, Metadata, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::Path,
};

use super::super::VaultResult;

pub(super) fn volume_and_file_id(_file: &File, metadata: &Metadata) -> io::Result<(u64, u128)> {
    Ok((metadata.dev(), metadata.ino().into()))
}

pub(super) fn delete_if_same(
    path: &Path,
    verify: impl FnOnce(&File) -> VaultResult<()>,
) -> VaultResult<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let folder = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let folder = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .open(folder)?;
    let name = c_name(name)?;
    let file = open_entry(&folder, &name)?;
    verify(&file)?;
    // SAFETY: unlinkat reads a NUL-terminated name relative to a live folder.
    if unsafe { libc::unlinkat(folder.as_raw_fd(), name.as_ptr(), 0) } < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput.into())
}

/// Opens the entry itself without following a link: on Linux as a path
/// only, so a link is opened as the link and fails the identity check.
fn open_entry(folder: &File, name: &CString) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    let access = libc::O_PATH;
    #[cfg(not(target_os = "linux"))]
    let access = libc::O_RDONLY | libc::O_NONBLOCK;
    // SAFETY: openat reads a NUL-terminated name and returns a new descriptor.
    let fd = unsafe {
        libc::openat(
            folder.as_raw_fd(),
            name.as_ptr(),
            access | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}
