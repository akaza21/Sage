use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::UNIX_EPOCH,
};

use uuid::Uuid;

use crate::{
    domain::{DocumentVersion, ResourceRef},
    editor_content::{MAX_EDITABLE_BYTES, detect_line_ending, fingerprint},
    editor_persistence::PersistedEditorBuffer,
    error::{AppError, AppResult},
    persistence::Database,
    workspace::Workspace,
};

pub(super) struct DiskDocument {
    pub(super) version: DocumentVersion,
    pub(super) canonical_content: String,
    pub(super) line_ending: String,
}

pub(super) fn read_disk_document(workspace: &Workspace, path: &str) -> AppResult<DiskDocument> {
    let absolute_path = workspace.resolve_existing_file(path)?;
    let metadata = absolute_path.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::InvalidRequest("resource is not a file".into()));
    }
    if metadata.len() > MAX_EDITABLE_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "file exceeds the {} MiB editor limit",
            MAX_EDITABLE_BYTES / 1024 / 1024
        )));
    }

    let bytes = fs::read(&absolute_path)?;
    if bytes.iter().take(8_192).any(|byte| *byte == 0) {
        return Err(AppError::InvalidRequest(
            "binary files cannot be opened in the editor".into(),
        ));
    }
    let raw_content = String::from_utf8(bytes.clone())
        .map_err(|_| AppError::InvalidRequest("file is not valid UTF-8".into()))?;
    let line_ending = detect_line_ending(&raw_content)?;
    let canonical_content = if line_ending == "crlf" {
        raw_content.replace("\r\n", "\n")
    } else {
        raw_content
    };

    Ok(DiskDocument {
        version: DocumentVersion {
            fingerprint: fingerprint(&bytes),
            byte_len: metadata.len(),
            modified_at: modified_at_millis(&metadata),
        },
        canonical_content,
        line_ending: line_ending.into(),
    })
}

pub(super) fn temporary_contract_path(path: &str) -> AppResult<String> {
    let (parent, file_name) = path.rsplit_once('/').map_or(("", path), |parts| parts);
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
    let absolute_temp = workspace.resolve_for_write(temp_path)?;
    match fs::remove_file(absolute_temp) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn write_temporary_file(target: &Path, temp: &Path, content: &[u8]) -> AppResult<()> {
    let permissions = target.metadata()?.permissions();
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(content)?;
    file.set_permissions(permissions)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn cleanup_failed_save(
    database: &Database,
    project_id: &str,
    resource: &ResourceRef,
    temp: &Path,
) {
    finish_failed_save_cleanup(database, project_id, resource, fs::remove_file(temp));
}

#[cfg(test)]
pub(super) fn cleanup_failed_save_with(
    database: &Database,
    project_id: &str,
    resource: &ResourceRef,
    temp: &Path,
    remove: impl FnOnce(&Path) -> std::io::Result<()>,
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

#[cfg(windows)]
pub(super) fn atomic_replace(target: &Path, temp: &Path) -> AppResult<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let temp_wide = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
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

#[cfg(not(windows))]
pub(super) fn atomic_replace(target: &Path, temp: &Path) -> AppResult<()> {
    fs::rename(temp, target)?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn sync_parent_directory(target: &Path) -> AppResult<()> {
    use std::fs::File;

    let parent = target
        .parent()
        .ok_or_else(|| AppError::InvalidRequest("file has no parent directory".into()))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn sync_parent_directory(_target: &Path) -> AppResult<()> {
    Ok(())
}

fn recorded_temp_is_valid(document_path: &str, temp_path: &str) -> bool {
    let (document_parent, file_name) = document_path
        .rsplit_once('/')
        .map_or(("", document_path), |parts| parts);
    let (temp_parent, temp_name) = temp_path
        .rsplit_once('/')
        .map_or(("", temp_path), |parts| parts);
    document_parent == temp_parent
        && temp_name.starts_with(&format!(".{file_name}.sage-"))
        && temp_name.ends_with(".tmp")
}
