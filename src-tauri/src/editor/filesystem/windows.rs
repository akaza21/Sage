//! Windows pins the root and every directory down to the parent with handles
//! that deny delete sharing, so no component can be renamed or replaced while
//! the save names entries beneath it.

use std::{
    fs::{File, OpenOptions},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS},
    Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
    },
};

use crate::{
    error::{AppError, AppResult},
    workspace::{
        RootIdentity, Workspace, open_held_parent, open_workspace_text_file, root_identity_file,
    },
};

pub(in crate::editor) struct HeldParent {
    guards: Vec<File>,
    directory: PathBuf,
    root: PathBuf,
    root_identity: RootIdentity,
    relative: String,
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

impl HeldParent {
    pub(in crate::editor) fn open(workspace: &Workspace, parent: &str) -> AppResult<Self> {
        let guards = open_held_parent(workspace.root(), workspace.root_identity(), parent)?;
        let directory = if parent.is_empty() {
            workspace.root().to_path_buf()
        } else {
            workspace.root().join(parent)
        };
        Ok(Self {
            guards,
            directory,
            root: workspace.root().to_path_buf(),
            root_identity: workspace.root_identity(),
            relative: parent.to_owned(),
        })
    }

    pub(in crate::editor) fn identity(&self) -> AppResult<RootIdentity> {
        root_identity_file(
            self.guards
                .last()
                .expect("the held walk always retains the root and parent"),
        )
    }

    fn entry(&self, name: &str) -> AppResult<PathBuf> {
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(AppError::InvalidRequest(
                "editor file name is not one path component".into(),
            ));
        }
        Ok(self.directory.join(name))
    }

    pub(in crate::editor) fn open_file(&self, name: &str) -> AppResult<File> {
        self.entry(name)?;
        let relative = if self.relative.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", self.relative)
        };
        open_workspace_text_file(&self.root, self.root_identity, &relative, None)
    }

    pub(super) fn create_new(&self, name: &str) -> AppResult<File> {
        Ok(OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.entry(name)?)?)
    }

    pub(in crate::editor) fn replace(&self, temp: &str, target: &str) -> AppResult<()> {
        let target_wide = wide(&self.entry(target)?);
        let temp_wide = wide(&self.entry(temp)?);
        // SAFETY: both path buffers are valid, NUL-terminated UTF-16 and live for the call.
        let replaced = unsafe {
            ReplaceFileW(
                target_wide.as_ptr(),
                temp_wide.as_ptr(),
                ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if replaced == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    /// A new File inherits its folder's permissions, as any new file does.
    pub(super) fn create_new_document(&self, name: &str) -> AppResult<File> {
        self.create_new(name)
    }

    /// Moves `temp` to `target` only if nothing has that name: `false` when
    /// something does, and then `temp` is left where it is.
    pub(in crate::editor) fn rename_new(&self, temp: &str, target: &str) -> AppResult<bool> {
        let target_wide = wide(&self.entry(target)?);
        let temp_wide = wide(&self.entry(temp)?);
        // Without MOVEFILE_REPLACE_EXISTING the move fails on an existing name.
        // SAFETY: both path buffers are valid, NUL-terminated UTF-16 and live for the call.
        let moved = unsafe {
            MoveFileExW(
                temp_wide.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved != 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(code)
                if code == ERROR_ALREADY_EXISTS as i32 || code == ERROR_FILE_EXISTS as i32 =>
            {
                Ok(false)
            }
            _ => Err(error.into()),
        }
    }

    pub(in crate::editor) fn remove(&self, name: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.entry(name).map_err(std::io::Error::other)?)
    }

    pub(in crate::editor) fn sync(&self) -> AppResult<()> {
        Ok(())
    }
}
