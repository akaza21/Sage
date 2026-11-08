//! A document's parent directory, opened once by the workspace's held-handle
//! walk. Every save step names entries relative to this handle, so replacing
//! the parent or any ancestor with a link after the walk cannot redirect the
//! save outside the directory that was checked.

use std::{
    ffi::CString,
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
};

use crate::{
    error::{AppError, AppResult},
    workspace::{RootIdentity, Workspace, open_held_parent, root_identity_metadata},
};

/// Owner read and write only until the target's permissions are copied.
const TEMP_FILE_MODE: libc::c_uint = 0o600;
/// A new File gets the usual mode, narrowed by the process umask.
const NEW_FILE_MODE: libc::c_uint = 0o666;
const READ_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
const CREATE_FLAGS: libc::c_int =
    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
const UNLINK_FILE: libc::c_int = 0;

pub(in crate::editor) struct HeldParent {
    directory: File,
}

fn entry_name(name: &str) -> AppResult<CString> {
    if name.is_empty() || name.contains('/') {
        return Err(AppError::InvalidRequest(
            "editor file name is not one path component".into(),
        ));
    }
    CString::new(name).map_err(|_| AppError::InvalidRequest("editor file name contains NUL".into()))
}

fn adopt(descriptor: libc::c_int) -> std::io::Result<File> {
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

impl HeldParent {
    pub(in crate::editor) fn open(workspace: &Workspace, parent: &str) -> AppResult<Self> {
        let directory = open_held_parent(workspace.root(), workspace.root_identity(), parent)?;
        Ok(Self { directory })
    }

    pub(in crate::editor) fn identity(&self) -> AppResult<RootIdentity> {
        root_identity_metadata(&self.directory.metadata()?)
    }

    /// Opens `name` for reading without following a final link.
    pub(in crate::editor) fn open_file(&self, name: &str) -> AppResult<File> {
        let name = entry_name(name)?;
        // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
        let descriptor =
            unsafe { libc::openat(self.directory.as_raw_fd(), name.as_ptr(), READ_FLAGS) };
        Ok(adopt(descriptor)?)
    }

    pub(super) fn create_new(&self, name: &str) -> AppResult<File> {
        self.create_new_with_mode(name, TEMP_FILE_MODE)
    }

    /// Like `create_new`, with the mode a new File gets (umask applies).
    pub(super) fn create_new_document(&self, name: &str) -> AppResult<File> {
        self.create_new_with_mode(name, NEW_FILE_MODE)
    }

    fn create_new_with_mode(&self, name: &str, mode: libc::c_uint) -> AppResult<File> {
        let name = entry_name(name)?;
        // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
        let descriptor = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                CREATE_FLAGS,
                mode,
            )
        };
        Ok(adopt(descriptor)?)
    }

    /// Atomically renames `temp` over `target`, both inside this directory.
    pub(in crate::editor) fn replace(&self, temp: &str, target: &str) -> AppResult<()> {
        let (temp, target) = (entry_name(temp)?, entry_name(target)?);
        let directory = self.directory.as_raw_fd();
        // SAFETY: both names are NUL-terminated and relative to the held descriptor.
        if unsafe { libc::renameat(directory, temp.as_ptr(), directory, target.as_ptr()) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    /// Renames `temp` to `target` only if nothing has that name: `false`
    /// when something does, and then `temp` is left where it is.
    pub(in crate::editor) fn rename_new(&self, temp: &str, target: &str) -> AppResult<bool> {
        let (temp, target) = (entry_name(temp)?, entry_name(target)?);
        let directory = self.directory.as_raw_fd();
        // SAFETY: both names are NUL-terminated and relative to the held descriptor.
        #[cfg(target_os = "linux")]
        let renamed = unsafe {
            libc::renameat2(
                directory,
                temp.as_ptr(),
                directory,
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        // SAFETY: as above.
        #[cfg(target_os = "macos")]
        let renamed = unsafe {
            libc::renameatx_np(
                directory,
                temp.as_ptr(),
                directory,
                target.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if renamed == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Ok(false);
        }
        #[cfg(target_os = "linux")]
        if error.raw_os_error() == Some(libc::EINVAL) {
            // The file system has no RENAME_NOREPLACE: a hard link refuses
            // an existing name too, and the temporary name then goes.
            // SAFETY: as above.
            let linked =
                unsafe { libc::linkat(directory, temp.as_ptr(), directory, target.as_ptr(), 0) };
            if linked < 0 {
                let error = std::io::Error::last_os_error();
                return match error.kind() {
                    std::io::ErrorKind::AlreadyExists => Ok(false),
                    _ => Err(error.into()),
                };
            }
            // SAFETY: as above.
            unsafe { libc::unlinkat(directory, temp.as_ptr(), UNLINK_FILE) };
            return Ok(true);
        }
        Err(error.into())
    }

    pub(in crate::editor) fn remove(&self, name: &str) -> std::io::Result<()> {
        let name = entry_name(name).map_err(std::io::Error::other)?;
        // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), UNLINK_FILE) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub(in crate::editor) fn sync(&self) -> AppResult<()> {
        self.directory.sync_all()?;
        Ok(())
    }
}
