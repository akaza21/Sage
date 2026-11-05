//! POSIX cannot delete through a descriptor, so the name is first moved
//! aside, atomically, to a private name in the same folder. Only the entry
//! moved aside is checked and removed: a file saved over the original name
//! after that move is never touched. An entry that fails the check is moved
//! back, without replacing anything saved there in the meantime.

use std::{
    ffi::{CStr, CString, OsStr},
    fs::{File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::Path,
};

use uuid::Uuid;

use super::super::VaultResult;

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
    let aside = c_name(aside_name(name))?;
    let name = c_name(name.as_bytes())?;
    rename_exclusive(&folder, &name, &aside)?;
    let checked = open_entry(&folder, &aside)
        .map_err(Into::into)
        .and_then(|file| verify(&file));
    if let Err(error) = checked {
        rename_exclusive(&folder, &aside, &name)?;
        return Err(error);
    }
    // SAFETY: unlinkat reads a NUL-terminated name relative to a live folder.
    if unsafe { libc::unlinkat(folder.as_raw_fd(), aside.as_ptr(), 0) } < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

/// A name no other program uses, beside the file and on its volume, so the
/// move aside is a rename within one folder.
fn aside_name(name: &OsStr) -> Vec<u8> {
    let mut aside = name.as_bytes().to_vec();
    aside.extend_from_slice(format!(".sage-delete-{}", Uuid::new_v4().simple()).as_bytes());
    aside
}

fn c_name(name: impl Into<Vec<u8>>) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::ErrorKind::InvalidInput.into())
}

/// Renames `from` to `to` in `folder`, failing if `to` exists.
fn rename_exclusive(folder: &File, from: &CStr, to: &CStr) -> io::Result<()> {
    let folder = folder.as_raw_fd();
    // SAFETY: both names are NUL-terminated and relative to a live folder.
    #[cfg(target_os = "linux")]
    let renamed = unsafe {
        libc::renameat2(
            folder,
            from.as_ptr(),
            folder,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    // SAFETY: as above.
    #[cfg(target_os = "macos")]
    let renamed = unsafe {
        libc::renameatx_np(
            folder,
            from.as_ptr(),
            folder,
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if renamed < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Opens the entry itself without following a link: on Linux as a path
/// only, so a link is opened as the link and fails the identity check.
fn open_entry(folder: &File, name: &CStr) -> io::Result<File> {
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
