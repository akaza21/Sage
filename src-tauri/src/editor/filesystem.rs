use std::{
    fs::{self, File},
    io::{Read, Write},
    time::UNIX_EPOCH,
};

use sage_core::error::{AppError, AppResult};
use uuid::Uuid;

use crate::{
    editor_content::{MAX_EDITABLE_BYTES, detect_line_ending, fingerprint},
    editor_persistence::PersistedEditorBuffer,
    persistence::Database,
    workspace::{RootIdentity, Workspace, leaf_identity, open_workspace_text_file},
};
use sage_domain::{editor::DocumentVersion, resource::ResourceRef};

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

/// Like `read_disk_document`, but a File (or folder) that does not exist is
/// `None`: a buffer can outlive its File, which was deleted outside Sage.
pub(super) fn read_disk_document_if_present(
    workspace: &Workspace,
    path: &str,
) -> AppResult<Option<DiskDocument>> {
    match read_disk_document(workspace, path) {
        Ok(document) => Ok(Some(document)),
        Err(error) if is_not_found(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn is_not_found(error: &AppError) -> bool {
    matches!(error, AppError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
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
    let parent = match HeldParent::open(workspace, parent) {
        Ok(parent) => parent,
        // The folder is gone, and the temporary file with it.
        Err(error) if is_not_found(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    match parent.remove(temp_name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

impl HeldParent {
    /// Reads the document `name` and returns it with the identity of the file
    /// read and the still open handle it was read through. On Windows that
    /// handle denies delete sharing, so while it is open the file cannot be
    /// deleted, replaced or renamed.
    pub(super) fn read_document_with_identity(
        &self,
        name: &str,
    ) -> AppResult<(DiskDocument, RootIdentity, File)> {
        let file = self.open_file(name)?;
        let identity = leaf_identity(&file, &file.metadata()?)?;
        Ok((disk_document_from(&file)?, identity, file))
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
    /// given permissions, makes its content durable and returns its identity.
    /// `before_sync` runs once the content is written and can abandon the
    /// write.
    pub(super) fn write_temporary_file(
        &self,
        temp_name: &str,
        permissions: fs::Permissions,
        content: &[u8],
        before_sync: impl FnOnce() -> std::io::Result<()>,
    ) -> AppResult<RootIdentity> {
        let mut file = self.create_new(temp_name)?;
        let identity = leaf_identity(&file, &file.metadata()?)?;
        file.write_all(content)?;
        before_sync()?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        Ok(identity)
    }
}

impl HeldParent {
    /// Creates `name` with `content`, only if nothing has that name yet:
    /// `false` when something does. The content is written to `temp_name`
    /// beside it and renamed in without replacing anything, so no reader sees
    /// a partial File and a File that appears meanwhile is never touched.
    /// The temporary file is removed whatever happens.
    pub(super) fn create_document(
        &self,
        name: &str,
        temp_name: &str,
        content: &[u8],
        before_rename: impl FnOnce() -> std::io::Result<()>,
    ) -> AppResult<bool> {
        let written = self.create_new_document(temp_name).and_then(|mut file| {
            file.write_all(content)?;
            before_rename()?;
            file.sync_all()?;
            Ok(())
        });
        let renamed = written.and_then(|()| self.rename_new(temp_name, name));
        // Gone after a rename; still there after a link, copy or failure.
        let _ = self.remove(temp_name);
        renamed
    }
}

/// Fails unless the canonical `path`, walked from the verified root, opens
/// the file with identity `expected`.
pub(super) fn ensure_path_names(
    workspace: &Workspace,
    path: &str,
    expected: RootIdentity,
) -> AppResult<()> {
    open_workspace_text_file(
        workspace.root(),
        workspace.root_identity(),
        path,
        Some(expected),
    )?;
    Ok(())
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "filesystem/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "filesystem/windows.rs"]
mod platform;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[path = "filesystem/other.rs"]
mod platform;
