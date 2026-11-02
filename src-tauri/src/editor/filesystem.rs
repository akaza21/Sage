use std::{
    fs::{self, File},
    io::{Read, Write},
    time::UNIX_EPOCH,
};

use uuid::Uuid;

use crate::{
    domain::{DocumentVersion, ResourceRef},
    editor_content::{MAX_EDITABLE_BYTES, detect_line_ending, fingerprint},
    editor_persistence::PersistedEditorBuffer,
    error::{AppError, AppResult},
    persistence::Database,
    workspace::{RootIdentity, Workspace, leaf_identity, open_workspace_text_file},
};

pub(super) use self::platform::HeldParent;

/// One byte past the editor cap, so a read that fills it proves the file is
/// too large whatever earlier metadata reported.
const EDITABLE_READ_PROBE_BYTES: u64 = MAX_EDITABLE_BYTES + 1;
/// Leading bytes inspected for a NUL before a file is treated as binary.
const BINARY_SNIFF_BYTES: usize = 8_192;
const BYTES_PER_MEBIBYTE: u64 = 1024 * 1024;

pub(super) struct DiskDocument {
    pub(super) version: DocumentVersion,
    pub(super) canonical_content: String,
    pub(super) line_ending: String,
}

/// Reads a document through one handle opened by the held-root walk.
pub(super) fn read_disk_document(workspace: &Workspace, path: &str) -> AppResult<DiskDocument> {
    let file = open_workspace_text_file(workspace.root(), workspace.root_identity(), path, None)?;
    disk_document_from(&file)
}

/// Builds the disk projection from an opened handle. The type check, the size
/// check and the read all use this handle; the size check only rejects early,
/// and the bounded read decides.
fn disk_document_from(file: &File) -> AppResult<DiskDocument> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::InvalidRequest("resource is not a file".into()));
    }
    if metadata.len() > MAX_EDITABLE_BYTES {
        return Err(oversize_error());
    }

    let bytes = read_editable_bytes(file)?;
    if bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0) {
        return Err(AppError::InvalidRequest(
            "binary files cannot be opened in the editor".into(),
        ));
    }
    let byte_len = bytes.len() as u64;
    let disk_fingerprint = fingerprint(&bytes);
    let raw_content = String::from_utf8(bytes)
        .map_err(|_| AppError::InvalidRequest("file is not valid UTF-8".into()))?;
    let line_ending = detect_line_ending(&raw_content)?;
    let canonical_content = if line_ending == "crlf" {
        raw_content.replace("\r\n", "\n")
    } else {
        raw_content
    };

    Ok(DiskDocument {
        version: DocumentVersion {
            fingerprint: disk_fingerprint,
            byte_len,
            modified_at: modified_at_millis(&metadata),
        },
        canonical_content,
        line_ending: line_ending.into(),
    })
}

/// Reads at most one byte past the editor cap and rejects anything larger.
pub(super) fn read_editable_bytes(reader: impl Read) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(EDITABLE_READ_PROBE_BYTES)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(oversize_error());
    }
    Ok(bytes)
}

fn oversize_error() -> AppError {
    AppError::ResourceLimit(format!(
        "file exceeds the {} MiB editor limit",
        MAX_EDITABLE_BYTES / BYTES_PER_MEBIBYTE
    ))
}

/// Splits a contract path into its parent directory and its final name.
pub(super) fn split_contract_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

pub(super) fn temporary_contract_path(path: &str) -> AppResult<String> {
    let (parent, file_name) = split_contract_path(path);
    if file_name.is_empty() {
        return Err(AppError::InvalidRequest(
            "file name must not be empty".into(),
        ));
    }
    let temp_name = format!(".{file_name}.sage-{}.tmp", Uuid::new_v4());
    Ok(if parent.is_empty() {
        temp_name
    } else {
        format!("{parent}/{temp_name}")
    })
}

pub(super) fn cleanup_recorded_temp(
    workspace: &Workspace,
    buffer: &PersistedEditorBuffer,
) -> AppResult<()> {
    let Some(temp_path) = &buffer.pending_temp_path else {
        return Ok(());
    };
    if !recorded_temp_is_valid(&buffer.resource.relative_path, temp_path) {
        return Err(AppError::InvalidRequest(
            "recorded editor temporary path is invalid".into(),
        ));
    }
    let (parent, temp_name) = split_contract_path(temp_path);
    match HeldParent::open(workspace, parent)?.remove(temp_name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

impl HeldParent {
    /// Reads the document `name` inside this directory.
    pub(super) fn read_document(&self, name: &str) -> AppResult<DiskDocument> {
        disk_document_from(&self.open_file(name)?)
    }

    /// Reads the document `name` and returns the identity of the file read.
    pub(super) fn read_document_with_identity(
        &self,
        name: &str,
    ) -> AppResult<(DiskDocument, RootIdentity)> {
        let file = self.open_file(name)?;
        let identity = leaf_identity(&file, &file.metadata()?)?;
        Ok((disk_document_from(&file)?, identity))
    }

    /// Fails unless `name` is still the regular file with identity `expected`.
    pub(super) fn ensure_entry_is(&self, name: &str, expected: RootIdentity) -> AppResult<()> {
        let file = self.open_file(name)?;
        if leaf_identity(&file, &file.metadata()?)? != expected {
            return Err(AppError::InvalidRequest(
                "the file was replaced while it was being saved".into(),
            ));
        }
        Ok(())
    }

    /// Fails unless `parent`, walked again from the verified root, still
    /// names this held directory.
    pub(super) fn ensure_named_by(&self, workspace: &Workspace, parent: &str) -> AppResult<()> {
        let still_named = HeldParent::open(workspace, parent)
            .and_then(|fresh| Ok(fresh.identity()? == self.identity()?));
        if !matches!(still_named, Ok(true)) {
            return Err(AppError::InvalidRequest(
                "the file's folder was moved or replaced while it was being saved".into(),
            ));
        }
        Ok(())
    }

    /// Creates the temporary file `temp_name` inside this directory with the
    /// given permissions and makes its content durable. `before_sync` runs
    /// once the content is written and can abandon the write.
    pub(super) fn write_temporary_file(
        &self,
        temp_name: &str,
        permissions: fs::Permissions,
        content: &[u8],
        before_sync: impl FnOnce() -> std::io::Result<()>,
    ) -> AppResult<()> {
        let mut file = self.create_new(temp_name)?;
        file.write_all(content)?;
        before_sync()?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        Ok(())
    }
}

pub(super) fn cleanup_failed_save(
    database: &Database,
    project_id: &str,
    resource: &ResourceRef,
    parent: &HeldParent,
    temp_name: &str,
) {
    finish_failed_save_cleanup(database, project_id, resource, parent.remove(temp_name));
}

#[cfg(test)]
pub(super) fn cleanup_failed_save_with(
    database: &Database,
    project_id: &str,
    resource: &ResourceRef,
    temp: &std::path::Path,
    remove: impl FnOnce(&std::path::Path) -> std::io::Result<()>,
) {
    finish_failed_save_cleanup(database, project_id, resource, remove(temp));
}

fn finish_failed_save_cleanup(
    database: &Database,
    project_id: &str,
    resource: &ResourceRef,
    removal: std::io::Result<()>,
) {
    match removal {
        Ok(()) => {
            let _ = database.clear_editor_pending_temp(project_id, resource);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let _ = database.clear_editor_pending_temp(project_id, resource);
        }
        Err(_) => {}
    }
}

fn modified_at_millis(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn recorded_temp_is_valid(document_path: &str, temp_path: &str) -> bool {
    let (document_parent, file_name) = split_contract_path(document_path);
    let (temp_parent, temp_name) = split_contract_path(temp_path);
    document_parent == temp_parent
        && temp_name.starts_with(&format!(".{file_name}.sage-"))
        && temp_name.ends_with(".tmp")
}

/// A document's parent directory, opened once by the workspace's held-handle
/// walk. Every save step names entries relative to this handle, so replacing
/// the parent or any ancestor with a link after the walk cannot redirect the
/// save outside the directory that was checked.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
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
        CString::new(name)
            .map_err(|_| AppError::InvalidRequest("editor file name contains NUL".into()))
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
            let name = entry_name(name)?;
            // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
            let descriptor = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    CREATE_FLAGS,
                    TEMP_FILE_MODE,
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

        pub(in crate::editor) fn remove(&self, name: &str) -> std::io::Result<()> {
            let name = entry_name(name).map_err(std::io::Error::other)?;
            // SAFETY: the held descriptor and NUL-terminated name are valid for the call.
            if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), UNLINK_FILE) } < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        pub(in crate::editor) fn sync(&self) -> AppResult<()> {
            self.directory.sync_all()?;
            Ok(())
        }
    }
}

/// Windows pins the root and every directory down to the parent with handles
/// that deny delete sharing, so no component can be renamed or replaced while
/// the save names entries beneath it.
#[cfg(windows)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        ptr,
    };

    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

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

        pub(in crate::editor) fn remove(&self, name: &str) -> std::io::Result<()> {
            std::fs::remove_file(self.entry(name).map_err(std::io::Error::other)?)
        }

        pub(in crate::editor) fn sync(&self) -> AppResult<()> {
            Ok(())
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use std::fs::File;

    use crate::{
        error::{AppError, AppResult},
        workspace::{RootIdentity, Workspace},
    };

    pub(in crate::editor) struct HeldParent;

    fn unavailable() -> AppError {
        AppError::Unsupported("safe editor saves are unavailable on this platform".into())
    }

    impl HeldParent {
        pub(in crate::editor) fn open(_workspace: &Workspace, _parent: &str) -> AppResult<Self> {
            Err(unavailable())
        }

        pub(in crate::editor) fn identity(&self) -> AppResult<RootIdentity> {
            Err(unavailable())
        }

        pub(in crate::editor) fn open_file(&self, _name: &str) -> AppResult<File> {
            Err(unavailable())
        }

        pub(super) fn create_new(&self, _name: &str) -> AppResult<File> {
            Err(unavailable())
        }

        pub(in crate::editor) fn replace(&self, _temp: &str, _target: &str) -> AppResult<()> {
            Err(unavailable())
        }

        pub(in crate::editor) fn remove(&self, _name: &str) -> std::io::Result<()> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }

        pub(in crate::editor) fn sync(&self) -> AppResult<()> {
            Err(unavailable())
        }
    }
}
