//! A document's parent directory, opened once by the workspace's held-handle
//! walk. Every save step names entries relative to this handle, so replacing
//! the parent or any ancestor with a link after the walk cannot redirect the
//! save outside the directory that was checked.

use std::{
    ffi::CString,
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
};

use sage_core::error::{AppError, AppResult};

use sage_workspace::{RootIdentity, Workspace, open_held_parent, root_identity_metadata};

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

fn unsupported(error: &std::io::Error) -> bool {
    // ENOTSUP and EOPNOTSUPP are one value on Linux and two on macOS.
    error.raw_os_error().is_some_and(|code| {
        code == libc::ENOSYS || code == libc::ENOTSUP || code == libc::EOPNOTSUPP
    })
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
    /// when something does. Where the file system cannot rename without
    /// replacing, it links `temp` in, and failing that copies it into a
    /// target it creates exclusively. `temp` stays for the caller to remove.
    pub(in crate::editor) fn rename_new(&self, temp: &str, target: &str) -> AppResult<bool> {
        let (temp_c, target_c) = (entry_name(temp)?, entry_name(target)?);
        let directory = self.directory.as_raw_fd();
        // SAFETY: both names are NUL-terminated and relative to the held descriptor.
        #[cfg(target_os = "linux")]
        let renamed = unsafe {
            libc::renameat2(
                directory,
                temp_c.as_ptr(),
                directory,
                target_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        // SAFETY: as above.
        #[cfg(target_os = "macos")]
        let renamed = unsafe {
            libc::renameatx_np(
                directory,
                temp_c.as_ptr(),
                directory,
                target_c.as_ptr(),
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
        // Old kernels, seccomp filters and some file systems have no
        // exclusive rename.
        if unsupported(&error) || error.raw_os_error() == Some(libc::EINVAL) {
            return self.link_new(temp, target);
        }
        Err(error.into())
    }

    /// Links `temp` in as `target` if nothing has that name, falling back to
    /// `copy_new` where the file system has no hard links.
    pub(in crate::editor) fn link_new(&self, temp: &str, target: &str) -> AppResult<bool> {
        let (temp_c, target_c) = (entry_name(temp)?, entry_name(target)?);
        let directory = self.directory.as_raw_fd();
        // SAFETY: both names are NUL-terminated and relative to the held descriptor.
        let linked =
            unsafe { libc::linkat(directory, temp_c.as_ptr(), directory, target_c.as_ptr(), 0) };
        if linked == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Ok(false);
        }
        // vfat, exFAT and some FUSE or SMB mounts refuse links.
        if unsupported(&error) || error.raw_os_error() == Some(libc::EPERM) {
            return self.copy_new(temp, target);
        }
        Err(error.into())
    }

    /// Creates `target` exclusively and copies `temp` into it. A target this
    /// call created and could not fill is removed, unless it was replaced.
    pub(in crate::editor) fn copy_new(&self, temp: &str, target: &str) -> AppResult<bool> {
        let mut source = self.open_file(temp)?;
        let mut file = match self.create_new_document(target) {
            Ok(file) => file,
            Err(AppError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        let copied = std::io::copy(&mut source, &mut file).and_then(|_| file.sync_all());
        if let Err(error) = copied {
            let ours = root_identity_metadata(&file.metadata()?)?;
            let still_ours = self
                .open_file(target)
                .and_then(|current| root_identity_metadata(&current.metadata()?))
                .is_ok_and(|current| current == ours);
            if still_ours {
                let _ = self.remove(target);
            }
            return Err(error.into());
        }
        Ok(true)
    }

    /// Creates the folder `name`; one that exists, or a link, fails with
    /// `AlreadyExists` and is never followed.
    pub(in crate::editor) fn create_folder(&self, name: &str) -> std::io::Result<()> {
        let name = entry_name(name).map_err(std::io::Error::other)?;
        // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
        if unsafe { libc::mkdirat(self.directory.as_raw_fd(), name.as_ptr(), 0o777) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Removes the empty folder `name`; one that is not empty stays.
    pub(in crate::editor) fn remove_folder(&self, name: &str) -> std::io::Result<()> {
        let name = entry_name(name).map_err(std::io::Error::other)?;
        // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
        if unsafe {
            libc::unlinkat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
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
