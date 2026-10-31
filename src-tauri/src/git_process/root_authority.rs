use std::{fs, path::Path};

use tokio::process::Command;

use crate::{
    error::{AppError, AppResult},
    workspace::Workspace,
};

pub(crate) struct GitRootAuthority {
    directory: fs::File,
    #[cfg(windows)]
    _anchor: fs::File,
}

impl GitRootAuthority {
    pub(crate) fn acquire(workspace: &Workspace) -> AppResult<Self> {
        let directory = open_directory(workspace.root())?;
        if directory_identity(&directory)? != workspace.root_identity() {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before Git access".into(),
            ));
        }
        let authority = Self {
            directory,
            #[cfg(windows)]
            _anchor: open_anchor(workspace.root())?,
        };
        #[cfg(windows)]
        workspace.ensure_root_identity()?;
        Ok(authority)
    }

    /// Duplicates the retained root handle so a second Git process can bind to
    /// the same identity-checked directory.
    pub(super) fn try_clone(&self) -> AppResult<Self> {
        Ok(Self {
            directory: self.directory.try_clone()?,
            #[cfg(windows)]
            _anchor: self._anchor.try_clone()?,
        })
    }

    pub(super) fn bind_child(&self, command: &mut Command, root: &Path) {
        bind_child(command, root, &self.directory);
    }
}

#[cfg(unix)]
fn open_directory(root: &Path) -> AppResult<fs::File> {
    Ok(fs::File::open(root)?)
}

#[cfg(unix)]
fn directory_identity(directory: &fs::File) -> AppResult<crate::workspace::RootIdentity> {
    crate::workspace::root_identity_metadata(&directory.metadata()?)
}

#[cfg(unix)]
fn bind_child(command: &mut Command, _root: &Path, directory: &fs::File) {
    use std::{os::fd::AsRawFd, os::unix::process::CommandExt};

    unsafe extern "C" {
        fn fchdir(fd: std::os::raw::c_int) -> std::os::raw::c_int;
    }

    let descriptor = directory.as_raw_fd();
    // SAFETY: the retained directory descriptor stays live until the child is
    // reaped. fchdir is the only operation performed between fork and exec.
    unsafe {
        command.as_std_mut().pre_exec(move || {
            if fchdir(descriptor) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(windows)]
fn open_directory(root: &Path) -> AppResult<fs::File> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    Ok(OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(root)?)
}

#[cfg(windows)]
fn open_anchor(root: &Path) -> AppResult<fs::File> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let dot_git = root.join(".git");
    let anchor = if dot_git.is_file() {
        dot_git
    } else {
        dot_git.join("HEAD")
    };
    Ok(OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(anchor)?)
}

#[cfg(windows)]
fn directory_identity(directory: &fs::File) -> AppResult<crate::workspace::RootIdentity> {
    crate::workspace::root_identity_file(directory)
}

#[cfg(windows)]
fn bind_child(command: &mut Command, root: &Path, _directory: &fs::File) {
    command.arg("-C").arg(root);
}

#[cfg(not(any(unix, windows)))]
fn open_directory(_root: &Path) -> AppResult<fs::File> {
    Err(AppError::InvalidRequest(
        "safe Git root authority is unavailable on this platform".into(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_directory: &fs::File) -> AppResult<crate::workspace::RootIdentity> {
    Err(AppError::InvalidRequest(
        "safe Git root authority is unavailable on this platform".into(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn bind_child(_command: &mut Command, _root: &Path, _directory: &fs::File) {}
