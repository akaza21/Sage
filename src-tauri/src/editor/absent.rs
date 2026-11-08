//! A buffer that outlives its File: the File was deleted outside Sage, and
//! the buffer is kept ("Keep mine") until a save creates the File again. Also
//! "Keep mine" over a File that changed on disk, which rebases the buffer.

use std::{
    collections::HashSet,
    sync::{Arc, LazyLock, Mutex, atomic::AtomicBool},
};

use super::filesystem::{
    DiskDocument, HeldParent, cleanup_failed_save, cleanup_recorded_temp, is_not_found,
    read_disk_document, read_disk_document_if_present, split_contract_path,
    temporary_contract_path,
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
    workspace::{Workspace, file_create::admit_created_path},
};

/// Files this process opened in the editor, by project and canonical path.
/// Only such a File, or one with a durable buffer, may take an absent base:
/// otherwise the WebView could create any file by naming a missing path.
static OPENED: LazyLock<Mutex<HashSet<(String, String, String)>>> = LazyLock::new(Mutex::default);
/// Bounds the set; past it, the set starts again, and a File opened before
/// that and deleted before any edit was persisted cannot be kept.
const MAX_OPENED: usize = 8192;

fn opened_key(workspace: &Workspace, resource: &ResourceRef) -> (String, String, String) {
    (
        workspace.project_id().into(),
        resource.root_id.clone(),
        resource.relative_path.clone(),
    )
}

/// Records that the editor opened `resource`, which existed or had a buffer.
pub(super) fn note_opened(workspace: &Workspace, resource: &ResourceRef) {
    let mut opened = OPENED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if opened.len() >= MAX_OPENED {
        opened.clear();
    }
    opened.insert(opened_key(workspace, resource));
}

fn was_opened(workspace: &Workspace, resource: &ResourceRef) -> bool {
    OPENED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&opened_key(workspace, resource))
}

/// Whether a snapshot with no durable buffer behind it may start one. The
/// base must be the File on disk, or, for a File this process opened and
/// that was deleted since, the version it was opened at; never absent.
pub(super) fn admits_unbuffered_snapshot(
    workspace: &Workspace,
    resource: &ResourceRef,
    base: &DocumentVersion,
    disk: Option<&DiskDocument>,
) -> bool {
    match disk {
        Some(disk) => disk.version.fingerprint == base.fingerprint,
        None => base.fingerprint != ABSENT_FINGERPRINT && was_opened(workspace, resource),
    }
}

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

/// The resource's canonical path. For a path that no longer exists, its
/// nearest existing folder must be spelled canonically; the missing names
/// are only checked for form. Two spellings of one missing name cannot both
/// hold a buffer: a buffer starts only from an opened File or a File on disk,
/// whose path is canonical.
pub(super) fn validated_path(workspace: &Workspace, resource: &ResourceRef) -> AppResult<String> {
    match workspace.validate_resource(resource, false) {
        Err(error) if is_not_found(&error) => {}
        result => return result,
    }
    let mut folder = split_contract_path(&resource.relative_path).0;
    loop {
        let candidate = ResourceRef {
            root_id: resource.root_id.clone(),
            relative_path: folder.into(),
        };
        match workspace.validate_resource(&candidate, true) {
            Ok(_) => return Ok(resource.relative_path.clone()),
            Err(error) if is_not_found(&error) && !folder.is_empty() => {
                folder = split_contract_path(folder).0;
            }
            Err(error) => return Err(error),
        }
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
    if disk.is_none() {
        // Keeping a deleted File means creating it on save.
        if durable_buffer.is_none() && !was_opened(workspace, &resource) {
            return Err(AppError::InvalidRequest(
                "only a File opened in the editor can be kept after it was deleted".into(),
            ));
        }
        admit_created_path(&resource.relative_path)?;
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
/// still not exist; if it does, the outcome is a conflict. Folders deleted
/// with it are created again, under the same rules as a new File.
pub(super) fn create_document(
    database: &Database,
    workspace: &Workspace,
    resource: ResourceRef,
    buffer: PersistedEditorBuffer,
) -> AppResult<EditorSaveResult> {
    create_document_with(database, workspace, resource, buffer, || Ok(()))
}

pub(super) fn create_document_with(
    database: &Database,
    workspace: &Workspace,
    resource: ResourceRef,
    buffer: PersistedEditorBuffer,
    before_rename: impl FnOnce() -> std::io::Result<()>,
) -> AppResult<EditorSaveResult> {
    let path = resource.relative_path.clone();
    admit_created_path(&path)?;
    let materialized = materialize_content(&buffer.content, &buffer.line_ending);
    reject_oversized_materialized(&materialized)?;
    let (parent_path, file_name) = split_contract_path(&path);
    ensure_folders(workspace, parent_path)?;
    let parent = HeldParent::open(workspace, parent_path)?;
    let temp_path = temporary_contract_path(&path)?;
    let (_, temp_name) = split_contract_path(&temp_path);
    database.set_editor_pending_temp(workspace.project_id(), &resource, &temp_path)?;
    let created =
        parent.create_document(file_name, temp_name, materialized.as_bytes(), before_rename);
    // The temporary file is gone either way; forget it.
    cleanup_failed_save(
        database,
        workspace.project_id(),
        &resource,
        &parent,
        temp_name,
    );
    if !created? {
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

/// Creates each missing folder of `parent_path`, top down, through the same
/// held, no-follow walk as Explorer's New Folder.
fn ensure_folders(workspace: &Workspace, parent_path: &str) -> AppResult<()> {
    if parent_path.is_empty() {
        return Ok(());
    }
    let mut existing = String::new();
    for name in parent_path.split('/') {
        let next = if existing.is_empty() {
            name.to_owned()
        } else {
            format!("{existing}/{name}")
        };
        match HeldParent::open(workspace, &next) {
            Ok(_) => {}
            Err(error) if is_not_found(&error) => {
                let parent = ResourceRef {
                    root_id: workspace.root_id().into(),
                    relative_path: existing.clone(),
                };
                let committed = Arc::new(AtomicBool::new(false));
                match workspace.create_directory_observed(&parent, name, committed) {
                    Ok(_) => {}
                    // Created meanwhile by someone else: use it.
                    Err(AppError::Io(error))
                        if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
        existing = next;
    }
    Ok(())
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
