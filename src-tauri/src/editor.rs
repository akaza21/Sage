mod absent;
mod filesystem;
mod replace;

use sage_core::error::{AppError, AppResult};

pub use self::replace::{DiskReplace, read_for_replace, replace_on_disk, replaced_file_text};

pub use self::absent::{ABSENT_FINGERPRINT, rebase_buffer};
use self::absent::{create_document, deleted_conflict, deleted_document, disk_version};
use self::filesystem::{
    DiskDocument, HeldParent, cleanup_failed_save, cleanup_recorded_temp, ensure_path_names,
    read_disk_document, read_disk_document_if_present, split_contract_path,
    temporary_contract_path,
};
use crate::{
    domain::{
        BufferSnapshotAck, BufferSnapshotRequest, DocumentVersion, EditorBufferVersion,
        EditorConflict, EditorDocument, EditorSaveResult, RecoveredEditorBuffer, ResourceRef,
    },
    editor_content::{
        canonicalize_editor_content, fingerprint, materialize_content,
        reject_oversized_materialized, reject_unsupported_content, validate_line_ending,
    },
    editor_persistence::PersistedEditorBuffer,
    persistence::Database,
    workspace::Workspace,
};

pub fn open_document(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<EditorDocument> {
    let resource = normalized_resource(workspace, resource)?;
    let buffer = database.editor_buffer(workspace.project_id(), &resource)?;
    let Some(buffer) = buffer else {
        let disk = read_disk_document(workspace, &resource.relative_path)?;
        absent::note_opened(workspace, &resource, &disk.version.fingerprint);
        return Ok(clean_document(resource, disk));
    };
    absent::note_opened(workspace, &resource, &buffer.base_fingerprint);
    validate_persisted_buffer(&buffer)?;
    let Some(disk) = read_disk_document_if_present(workspace, &resource.relative_path)? else {
        return Ok(buffered_document(resource, None, buffer));
    };

    if disk.version.fingerprint == buffer.buffer_fingerprint {
        cleanup_recorded_temp(workspace, &buffer)?;
        database.delete_editor_buffer(workspace.project_id(), &resource)?;
        return Ok(clean_document(resource, disk));
    }

    Ok(buffered_document(resource, Some(disk), buffer))
}

pub fn snapshot_buffer(
    database: &Database,
    workspace: &Workspace,
    mut request: BufferSnapshotRequest,
) -> AppResult<BufferSnapshotAck> {
    request.resource = normalized_resource(workspace, &request.resource)?;
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

    let durable_buffer = database.editor_buffer(workspace.project_id(), &request.resource)?;
    let disk = read_disk_document_if_present(workspace, &request.resource.relative_path)?;
    let disk_matches_base = disk_version(disk.as_ref()).fingerprint == request.base.fingerprint;
    if durable_buffer.is_none()
        && !absent::admits_unbuffered_snapshot(
            workspace,
            &request.resource,
            &request.base,
            disk.as_ref(),
        )
    {
        return Err(AppError::InvalidRequest(
            "snapshot base no longer matches disk and no durable buffer exists".into(),
        ));
    }
    if disk_matches_base && buffer_fingerprint == request.base.fingerprint {
        return database.apply_clean_editor_snapshot(workspace.project_id(), &request);
    }

    database.persist_editor_buffer(
        workspace.project_id(),
        &request,
        &request.content,
        &buffer_fingerprint,
    )
}

pub fn save_document(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<EditorSaveResult> {
    save_document_with(database, workspace, resource, |_| Ok(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveStage {
    ParentHeld,
    TempWritten,
    BeforeReplace,
    AfterReplace,
    BeforeRecoveryClear,
}

fn save_document_with(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
    mut observe: impl FnMut(SaveStage) -> std::io::Result<()>,
) -> AppResult<EditorSaveResult> {
    let resource = normalized_resource(workspace, resource)?;
    let path = resource.relative_path.clone();
    let Some(buffer) = database.editor_buffer(workspace.project_id(), &resource)? else {
        return Ok(EditorSaveResult {
            outcome: "unchanged".into(),
            document: clean_document(resource, read_disk_document(workspace, &path)?),
        });
    };
    validate_persisted_buffer(&buffer)?;

    let disk = read_disk_document_if_present(workspace, &path)?;
    if buffer.base_fingerprint == ABSENT_FINGERPRINT {
        return match disk {
            None => create_document(database, workspace, resource, buffer),
            Some(disk) => Ok(conflict_result(resource, Some(disk), buffer)),
        };
    }
    let disk = match disk {
        Some(disk) if disk.version.fingerprint == buffer.base_fingerprint => disk,
        disk => return Ok(conflict_result(resource, disk, buffer)),
    };

    let materialized = materialize_content(&buffer.content, &buffer.line_ending);
    reject_oversized_materialized(&materialized)?;
    if fingerprint(materialized.as_bytes()) != buffer.buffer_fingerprint {
        return Err(AppError::InvalidRequest(
            "durable editor buffer fingerprint is inconsistent".into(),
        ));
    }
    if buffer.buffer_fingerprint == buffer.base_fingerprint {
        database.delete_editor_buffer(workspace.project_id(), &resource)?;
        return Ok(EditorSaveResult {
            outcome: "unchanged".into(),
            document: clean_document(resource, disk),
        });
    }

    let (parent_path, file_name) = split_contract_path(&path);
    let parent = HeldParent::open(workspace, parent_path)?;
    observe(SaveStage::ParentHeld)?;
    // The target handle closes here, so none is open across the replace.
    let permissions = parent.open_file(file_name)?.metadata()?.permissions();
    let temp_path = temporary_contract_path(&path)?;
    let (_, temp_name) = split_contract_path(&temp_path);
    database.set_editor_pending_temp(workspace.project_id(), &resource, &temp_path)?;
    let abandon = || {
        cleanup_failed_save(
            database,
            workspace.project_id(),
            &resource,
            &parent,
            temp_name,
        )
    };

    let installed =
        match parent.write_temporary_file(temp_name, permissions, materialized.as_bytes(), || {
            observe(SaveStage::TempWritten)
        }) {
            Ok(identity) => identity,
            Err(error) => {
                abandon();
                return Err(error);
            }
        };

    let (latest_disk, target_identity, target) = match parent.read_document_with_identity(file_name)
    {
        Ok(read) => read,
        Err(error) => {
            abandon();
            return Err(error);
        }
    };
    // Windows cannot replace a file while this handle is open.
    drop(target);
    if latest_disk.version.fingerprint != buffer.base_fingerprint {
        abandon();
        return Ok(conflict_result(resource, Some(latest_disk), buffer));
    }

    let replaced = observe(SaveStage::BeforeReplace)
        .map_err(AppError::from)
        .and_then(|()| parent.ensure_entry_is(file_name, target_identity))
        .and_then(|()| parent.ensure_named_by(workspace, parent_path))
        .and_then(|()| parent.replace(temp_name, file_name));
    if let Err(error) = replaced {
        abandon();
        return Err(error);
    }
    database.clear_editor_pending_temp(workspace.project_id(), &resource)?;
    parent.sync()?;

    // The recovery buffer is cleared only once the installed file is proven:
    // first through the held directory (its identity and bytes), then by
    // path from the verified root (the parent and the target). Until then a
    // replaced or deleted target, or a moved parent, keeps the buffer.
    let verified = observe(SaveStage::AfterReplace)
        .map_err(AppError::from)
        .and_then(|()| parent.read_document_with_identity(file_name))
        .and_then(|(saved, identity, handle)| {
            if identity != installed || saved.version.fingerprint != buffer.buffer_fingerprint {
                return Err(AppError::InvalidRequest(
                    "the saved file was replaced".into(),
                ));
            }
            parent.ensure_named_by(workspace, parent_path)?;
            ensure_path_names(workspace, &path, installed)?;
            Ok((saved, handle))
        });
    let Ok((saved, _verified_target)) = verified else {
        return Err(AppError::InvalidRequest(
            "the content was written, but the file's location changed during the save or \
             could not be verified; the unsaved buffer is kept"
                .into(),
        ));
    };
    // The verified target handle and, on Windows, the parent guards stay open
    // until the recovery delete commits, so on Windows nothing can delete,
    // replace or rename the saved file or its folders in between. Unix
    // descriptors cannot prevent that; the bytes were verified at this path,
    // so a rename or delete from here on is the user moving the file after a
    // successful save and does not undo the save.
    observe(SaveStage::BeforeRecoveryClear)?;
    database.delete_editor_buffer(workspace.project_id(), &resource)?;

    Ok(EditorSaveResult {
        outcome: "saved".into(),
        document: clean_document(resource, saved),
    })
}

pub fn discard_document(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<EditorDocument> {
    let resource = normalized_resource(workspace, resource)?;
    if let Some(buffer) = database.editor_buffer(workspace.project_id(), &resource)? {
        cleanup_recorded_temp(workspace, &buffer)?;
    }
    database.delete_editor_buffer(workspace.project_id(), &resource)?;
    Ok(
        match read_disk_document_if_present(workspace, &resource.relative_path)? {
            Some(disk) => clean_document(resource, disk),
            None => deleted_document(resource),
        },
    )
}

pub fn revert_document(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<EditorDocument> {
    let resource = normalized_resource(workspace, resource)?;
    if read_disk_document_if_present(workspace, &resource.relative_path)?.is_some() {
        return discard_document(database, workspace, &resource);
    }
    match database.editor_buffer(workspace.project_id(), &resource)? {
        Some(buffer) => {
            validate_persisted_buffer(&buffer)?;
            Ok(buffered_document(resource, None, buffer))
        }
        None => Ok(deleted_document(resource)),
    }
}

pub fn recover_pending_saves(database: &Database, workspace: &Workspace) -> AppResult<()> {
    for buffer in
        database.dirty_editor_buffers(workspace.project_id(), Some(workspace.root_id()))?
    {
        let Some(_) = &buffer.pending_temp_path else {
            continue;
        };
        validate_persisted_buffer(&buffer)?;
        cleanup_recorded_temp(workspace, &buffer)?;
        if read_disk_document(workspace, &buffer.resource.relative_path)
            .is_ok_and(|disk| disk.version.fingerprint == buffer.buffer_fingerprint)
        {
            database.delete_editor_buffer(workspace.project_id(), &buffer.resource)?;
        } else {
            database.clear_editor_pending_temp(workspace.project_id(), &buffer.resource)?;
        }
    }
    Ok(())
}

pub fn recovered_buffers(
    database: &Database,
    project_id: &str,
) -> AppResult<Vec<RecoveredEditorBuffer>> {
    database
        .dirty_editor_buffers(project_id, None)
        .map(|buffers| {
            buffers
                .into_iter()
                .map(|buffer| RecoveredEditorBuffer {
                    resource: buffer.resource,
                    revision: buffer.revision,
                })
                .collect()
        })
}

fn clean_document(resource: ResourceRef, disk: DiskDocument) -> EditorDocument {
    EditorDocument {
        resource,
        base: disk.version.clone(),
        buffer: EditorBufferVersion {
            fingerprint: disk.version.fingerprint.clone(),
            byte_len: disk.canonical_content.len() as u64,
            revision: 0,
            content: disk.canonical_content,
        },
        line_ending: disk.line_ending,
        dirty: false,
        conflict: None,
        disk: disk.version,
    }
}

fn buffered_document(
    resource: ResourceRef,
    disk: Option<DiskDocument>,
    buffer: PersistedEditorBuffer,
) -> EditorDocument {
    let disk = disk_version(disk.as_ref());
    let conflict = (disk.fingerprint != buffer.base_fingerprint).then(|| {
        if disk.fingerprint == ABSENT_FINGERPRINT {
            deleted_conflict()
        } else {
            external_change_conflict()
        }
    });
    EditorDocument {
        resource,
        disk,
        base: DocumentVersion {
            fingerprint: buffer.base_fingerprint,
            byte_len: buffer.base_byte_len,
            modified_at: buffer.base_modified_at,
        },
        buffer: EditorBufferVersion {
            fingerprint: buffer.buffer_fingerprint,
            byte_len: buffer.byte_len,
            revision: buffer.revision,
            content: buffer.content,
        },
        line_ending: buffer.line_ending,
        dirty: true,
        conflict,
    }
}

fn external_change_conflict() -> EditorConflict {
    EditorConflict {
        kind: "external-change".into(),
        message: "The file changed on disk after this buffer was opened. Reload it, keep your \
                  version, or compare the two."
            .into(),
    }
}

fn conflict_result(
    resource: ResourceRef,
    disk: Option<DiskDocument>,
    buffer: PersistedEditorBuffer,
) -> EditorSaveResult {
    EditorSaveResult {
        outcome: "conflict".into(),
        document: buffered_document(resource, disk, buffer),
    }
}

fn normalized_resource(workspace: &Workspace, resource: &ResourceRef) -> AppResult<ResourceRef> {
    Ok(ResourceRef {
        root_id: workspace.root_id().into(),
        relative_path: absent::validated_path(workspace, resource)?,
    })
}

fn validate_persisted_buffer(buffer: &PersistedEditorBuffer) -> AppResult<()> {
    validate_line_ending(&buffer.line_ending)?;
    reject_unsupported_content(&buffer.content)?;
    if buffer.byte_len != buffer.content.len() as u64 {
        return Err(AppError::InvalidRequest(
            "durable editor buffer byte length is inconsistent".into(),
        ));
    }
    let materialized = materialize_content(&buffer.content, &buffer.line_ending);
    reject_oversized_materialized(&materialized)?;
    if fingerprint(materialized.as_bytes()) != buffer.buffer_fingerprint {
        return Err(AppError::InvalidRequest(
            "durable editor buffer fingerprint is inconsistent".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
