//! Workspace Replace on disk: one File at a time, through the same held
//! parent walk, limits and atomic replace as a save.

use super::{
    filesystem::{
        HeldParent, ensure_path_names, read_disk_document, split_contract_path,
        temporary_contract_path,
    },
    normalized_resource,
};
use crate::{
    domain::ResourceRef,
    editor_content::{
        fingerprint, materialize_content, reject_oversized_materialized, reject_unsupported_content,
    },
    editor_persistence::PersistedEditorBuffer,
    error::{AppError, AppResult},
    persistence::Database,
    workspace::{Workspace, directory_skip::protected_workspace_name},
};

/// A File as a Replace reads it: its fingerprint and its text with `\n`
/// line ends.
pub struct ReplaceSource {
    pub resource: ResourceRef,
    pub fingerprint: String,
    pub content: String,
}

/// What a Replace did to one File.
#[derive(Debug, PartialEq, Eq)]
pub enum DiskReplace {
    Written {
        fingerprint: String,
        count: u64,
    },
    /// The File is not as it was when the Replace was previewed; nothing was
    /// written.
    Stale,
    /// Nothing in the File matches now; nothing was written.
    NoMatches,
}

/// Reads `resource` for a Replace, refusing what a Replace may not write.
pub fn read_for_replace(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<ReplaceSource> {
    let resource = admitted(database, workspace, resource)?;
    let disk = read_disk_document(workspace, &resource.relative_path)?;
    Ok(ReplaceSource {
        resource,
        fingerprint: disk.version.fingerprint,
        content: disk.canonical_content,
    })
}

/// Writes what `edit` makes of the File's text, if the File still has
/// `expected_fingerprint`, by replacing the whole File at once: a reader
/// sees the old File or the new one, never a mix. `edit` returns the new
/// text, with `\n` line ends, and how many matches it replaced.
pub fn replace_on_disk(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
    expected_fingerprint: &str,
    edit: impl FnOnce(&str) -> AppResult<(String, u64)>,
) -> AppResult<DiskReplace> {
    replace_on_disk_with(
        database,
        workspace,
        resource,
        expected_fingerprint,
        edit,
        |_| Ok(()),
    )
}

/// The points in a Replace at which `replace_on_disk_with` calls its
/// observer, as a save's stages do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplaceStage {
    /// The recovery record names the temporary file; nothing is written.
    Recorded,
    /// The temporary file holds the replaced text but is not yet durable.
    TempWritten,
    /// The temporary file is durable; the second stale check is next.
    BeforeStaleCheck,
    /// The stale check has passed and the swap is next.
    BeforeReplace,
    /// The temporary file has replaced the File; nothing is verified yet.
    AfterReplace,
}

/// `replace_on_disk`, failing at a stage where `observe` returns an error,
/// as the step there would.
pub(super) fn replace_on_disk_with(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
    expected_fingerprint: &str,
    edit: impl FnOnce(&str) -> AppResult<(String, u64)>,
    mut observe: impl FnMut(ReplaceStage) -> std::io::Result<()>,
) -> AppResult<DiskReplace> {
    let resource = admitted(database, workspace, resource)?;
    let project_id = workspace.project_id();
    let path = resource.relative_path.clone();
    let disk = read_disk_document(workspace, &path)?;
    if disk.version.fingerprint != expected_fingerprint {
        return Ok(DiskReplace::Stale);
    }
    let (content, count) = edit(&disk.canonical_content)?;
    if count == 0 {
        return Ok(DiskReplace::NoMatches);
    }
    reject_unsupported_content(&content)?;
    // The text read has `\n` line ends and the replacement holds no line
    // break, so a carriage return here would be a stray one.
    if content.contains('\r') {
        return Err(AppError::InvalidRequest(
            "the replaced text would hold a stray carriage return".into(),
        ));
    }
    let materialized = materialize_content(&content, &disk.line_ending);
    reject_oversized_materialized(&materialized)?;
    let replaced_fingerprint = fingerprint(materialized.as_bytes());

    let (parent_path, file_name) = split_contract_path(&path);
    let parent = HeldParent::open(workspace, parent_path)?;
    let permissions = parent.open_file(file_name)?.metadata()?.permissions();
    let temp_path = temporary_contract_path(&path)?;
    let (_, temp_name) = split_contract_path(&temp_path);
    // Recorded before the temporary file exists, as a save records it, so a
    // crash from here on leaves startup recovery a temporary file to remove.
    database.record_replace_in_progress(
        project_id,
        &PersistedEditorBuffer {
            resource: resource.clone(),
            base_fingerprint: disk.version.fingerprint.clone(),
            base_byte_len: disk.version.byte_len,
            base_modified_at: disk.version.modified_at,
            line_ending: disk.line_ending.clone(),
            byte_len: content.len() as u64,
            content,
            buffer_fingerprint: replaced_fingerprint.clone(),
            revision: 1,
            pending_temp_path: Some(temp_path.clone()),
        },
    )?;
    // Removes the temporary file, and the record once nothing is left for
    // recovery to remove.
    let abandon = || match parent.remove(temp_name) {
        Ok(()) => {
            let _ = database.delete_editor_buffer(project_id, &resource);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let _ = database.delete_editor_buffer(project_id, &resource);
        }
        Err(_) => {}
    };
    if let Err(error) = observe(ReplaceStage::Recorded) {
        abandon();
        return Err(error.into());
    }

    let installed =
        match parent.write_temporary_file(temp_name, permissions, materialized.as_bytes(), || {
            observe(ReplaceStage::TempWritten)
        }) {
            Ok(identity) => identity,
            Err(error) => {
                abandon();
                return Err(error);
            }
        };
    // Read again just before the replace: a write since the first read
    // makes the matches stale.
    let latest = observe(ReplaceStage::BeforeStaleCheck)
        .map_err(AppError::from)
        .and_then(|()| parent.read_document_with_identity(file_name));
    let (latest, identity, target) = match latest {
        Ok(read) => read,
        Err(error) => {
            abandon();
            return Err(error);
        }
    };
    // Windows cannot replace a file while this handle is open.
    drop(target);
    if latest.version.fingerprint != expected_fingerprint {
        abandon();
        return Ok(DiskReplace::Stale);
    }
    let replaced = observe(ReplaceStage::BeforeReplace)
        .map_err(AppError::from)
        .and_then(|()| parent.ensure_entry_is(file_name, identity))
        .and_then(|()| parent.ensure_named_by(workspace, parent_path))
        .and_then(|()| parent.replace(temp_name, file_name));
    if let Err(error) = replaced {
        abandon();
        return Err(error);
    }
    // The File is replaced from here on: what follows can only fail to
    // prove it, never undo it, so none of it reports the File as refused.
    if let Err(error) = parent.sync() {
        tracing::warn!(%error, "replaced File's folder could not be synced");
    }
    // As a save does, the record is cleared only once the installed File is
    // proven, through the held folder (identity and bytes) and by path from
    // the verified root. Until then it stays, as unsaved edits holding the
    // replaced text, and recovery settles it.
    let verified = observe(ReplaceStage::AfterReplace)
        .map_err(AppError::from)
        .and_then(|()| parent.read_document_with_identity(file_name))
        .and_then(|(written, identity, handle)| {
            if identity != installed || written.version.fingerprint != replaced_fingerprint {
                return Err(AppError::InvalidRequest(
                    "the replaced File was changed again".into(),
                ));
            }
            parent.ensure_named_by(workspace, parent_path)?;
            ensure_path_names(workspace, &path, installed)?;
            Ok(handle)
        });
    let Ok(_verified_target) = verified else {
        return Err(AppError::InvalidRequest(
            "the replacement was written, but the File's location changed or could not be \
             verified; it is kept as unsaved edits"
                .into(),
        ));
    };
    // A record left behind matches the File on disk, so recovery drops it.
    if let Err(error) = database.delete_editor_buffer(project_id, &resource) {
        tracing::warn!(%error, "replace recovery record could not be cleared");
    }
    Ok(DiskReplace::Written {
        fingerprint: replaced_fingerprint,
        count,
    })
}

/// The canonical resource, if a Replace may write it: not inside a
/// protected folder such as `.git`, and with no unsaved editor buffer,
/// which the editor changes instead.
fn admitted(
    database: &Database,
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<ResourceRef> {
    let resource = normalized_resource(workspace, resource)?;
    if resource
        .relative_path
        .split('/')
        .any(protected_workspace_name)
    {
        return Err(AppError::InvalidRequest(
            "Sage doesn't replace in protected folders such as .git".into(),
        ));
    }
    if database
        .editor_buffer(workspace.project_id(), &resource)?
        .is_some()
    {
        return Err(AppError::InvalidRequest(
            "it has unsaved changes in the editor".into(),
        ));
    }
    Ok(resource)
}

#[cfg(test)]
#[path = "replace_tests.rs"]
mod tests;
