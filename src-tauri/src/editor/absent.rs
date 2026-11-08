//! A buffer that outlives its File: the File was deleted outside Sage, and
//! the buffer is kept ("Keep mine") until a save creates the File again. Also
//! "Keep mine" over a File that changed on disk, which rebases the buffer.

use super::filesystem::{
    DiskDocument, HeldParent, cleanup_recorded_temp, read_disk_document,
    read_disk_document_if_present, split_contract_path,
};
use super::{
    buffered_document, clean_document, conflict_result, normalized_resource, open_document,
};
use crate::{
    domain::{
        BufferRebaseRequest, BufferSnapshotRequest, DocumentVersion, EditorBufferVersion,
        EditorConflict, EditorDocument, EditorSaveResult, ResourceRef,
    },
    editor_content::{
        canonicalize_editor_content, fingerprint, materialize_content,
        reject_oversized_materialized, reject_unsupported_content, validate_line_ending,
    },
    editor_persistence::PersistedEditorBuffer,
    error::{AppError, AppResult},
    persistence::Database,
    workspace::Workspace,
};

/// The base of a buffer whose File does not exist on disk: kept over a
/// deletion, its save creates the File. Real fingerprints are hex digests, so
/// this never matches one.
pub const ABSENT_FINGERPRINT: &str = "absent";

pub(super) fn absent_version() -> DocumentVersion {
    DocumentVersion {
        fingerprint: ABSENT_FINGERPRINT.into(),
        byte_len: 0,
        modified_at: None,
    }
}

pub(super) fn disk_version(disk: Option<&DiskDocument>) -> DocumentVersion {
    disk.map_or_else(absent_version, |disk| disk.version.clone())
}

/// The resource's canonical path. One that no longer exists is accepted on
/// its spelling, which `validate_resource` has already checked by the time
/// it looks for the File; every later access walks the held root.
pub(super) fn validated_path(workspace: &Workspace, resource: &ResourceRef) -> AppResult<String> {
    match workspace.validate_resource(resource, false) {
        Err(AppError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(resource.relative_path.clone())
        }
        result => result,
    }
}

/// "Keep mine": puts `content` on top of the File as it is on disk, if that is
/// still the version the user saw (`expected_disk`). Otherwise nothing changes
/// and the outcome is a conflict with the File as it is now.
pub fn rebase_buffer(
    database: &Database,
    workspace: &Workspace,
    mut request: BufferRebaseRequest,
) -> AppResult<EditorSaveResult> {
    let resource = normalized_resource(workspace, &request.resource)?;
    if request.revision == 0 {
        return Err(AppError::InvalidRequest(
            "buffer revision must be greater than zero".into(),
        ));
    }
    validate_line_ending(&request.line_ending)?;
    request.content = canonicalize_editor_content(&request.content);
    reject_unsupported_content(&request.content)?;
    let materialized = materialize_content(&request.content, &request.line_ending);
    reject_oversized_materialized(&materialized)?;
    let buffer_fingerprint = fingerprint(materialized.as_bytes());

    let durable_buffer = database.editor_buffer(workspace.project_id(), &resource)?;
    let disk = read_disk_document_if_present(workspace, &resource.relative_path)?;
    let base = disk_version(disk.as_ref());
    if base.fingerprint != request.expected_disk.fingerprint {
        let document = match (disk, durable_buffer) {
            (disk, Some(buffer)) => buffered_document(resource, disk, buffer),
            (Some(disk), None) => clean_document(resource, disk),
            (None, None) => deleted_document(resource),
        };
        return Ok(EditorSaveResult {
            outcome: "conflict".into(),
            document,
        });
    }
    if let Some(buffer) = &durable_buffer {
        cleanup_recorded_temp(workspace, buffer)?;
    }
    if let Some(disk) = disk.filter(|disk| disk.version.fingerprint == buffer_fingerprint) {
        database.delete_editor_buffer(workspace.project_id(), &resource)?;
        return Ok(EditorSaveResult {
            outcome: "rebased".into(),
            document: clean_document(resource, disk),
        });
    }
    let snapshot = BufferSnapshotRequest {
        resource: resource.clone(),
        base,
        line_ending: request.line_ending,
        content: request.content,
        revision: request.revision,
    };
    database.rebase_editor_buffer(
        workspace.project_id(),
        &snapshot,
        &snapshot.content,
        &buffer_fingerprint,
    )?;
    open_document(database, workspace, &resource).map(|document| EditorSaveResult {
        outcome: "rebased".into(),
        document,
    })
}

/// Saves a buffer kept over a deletion by creating its File, which must
/// still not exist; if it does, the outcome is a conflict.
pub(super) fn create_document(
    database: &Database,
    workspace: &Workspace,
    resource: ResourceRef,
    buffer: PersistedEditorBuffer,
) -> AppResult<EditorSaveResult> {
    let path = resource.relative_path.clone();
    let materialized = materialize_content(&buffer.content, &buffer.line_ending);
    reject_oversized_materialized(&materialized)?;
    let (parent_path, file_name) = split_contract_path(&path);
    let parent = HeldParent::open(workspace, parent_path)?;
    if parent
        .create_document(file_name, materialized.as_bytes())?
        .is_none()
    {
        let disk = read_disk_document_if_present(workspace, &path)?;
        return Ok(conflict_result(resource, disk, buffer));
    }
    parent.sync()?;
    let saved = read_disk_document(workspace, &path)?;
    if saved.version.fingerprint != buffer.buffer_fingerprint {
        return Err(AppError::InvalidRequest(
            "the file was created, but it changed before it could be verified; the unsaved \
             buffer is kept"
                .into(),
        ));
    }
    database.delete_editor_buffer(workspace.project_id(), &resource)?;
    Ok(EditorSaveResult {
        outcome: "saved".into(),
        document: clean_document(resource, saved),
    })
}

/// A File with no buffer that is not on disk, as a discard leaves it.
pub(super) fn deleted_document(resource: ResourceRef) -> EditorDocument {
    EditorDocument {
        resource,
        disk: absent_version(),
        base: absent_version(),
        buffer: EditorBufferVersion {
            fingerprint: ABSENT_FINGERPRINT.into(),
            byte_len: 0,
            revision: 0,
            content: String::new(),
        },
        line_ending: "none".into(),
        dirty: false,
        conflict: Some(deleted_conflict()),
    }
}

pub(super) fn deleted_conflict() -> EditorConflict {
    EditorConflict {
        kind: "deleted".into(),
        message: "This file was deleted outside Sage.".into(),
    }
}
