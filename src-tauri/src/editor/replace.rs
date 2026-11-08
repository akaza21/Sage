//! Workspace Replace on disk: one File at a time, through the same held
//! parent walk, limits and atomic replace as a save.

use super::{
    filesystem::{HeldParent, read_disk_document, split_contract_path, temporary_contract_path},
    normalized_resource,
};
use crate::{
    domain::ResourceRef,
    editor_content::{
        fingerprint, materialize_content, reject_oversized_materialized, reject_unsupported_content,
    },
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
    edit: impl FnOnce(&str) -> (String, u64),
) -> AppResult<DiskReplace> {
    let resource = admitted(database, workspace, resource)?;
    let path = resource.relative_path.as_str();
    let disk = read_disk_document(workspace, path)?;
    if disk.version.fingerprint != expected_fingerprint {
        return Ok(DiskReplace::Stale);
    }
    let (content, count) = edit(&disk.canonical_content);
    if count == 0 {
        return Ok(DiskReplace::NoMatches);
    }
    reject_unsupported_content(&content)?;
    let materialized = materialize_content(&content, &disk.line_ending);
    reject_oversized_materialized(&materialized)?;

    let (parent_path, file_name) = split_contract_path(path);
    let parent = HeldParent::open(workspace, parent_path)?;
    let permissions = parent.open_file(file_name)?.metadata()?.permissions();
    let temp_path = temporary_contract_path(path)?;
    let (_, temp_name) = split_contract_path(&temp_path);
    let abandon = || {
        let _ = parent.remove(temp_name);
    };
    if let Err(error) =
        parent.write_temporary_file(temp_name, permissions, materialized.as_bytes(), || Ok(()))
    {
        abandon();
        return Err(error);
    }
    // Read again just before the replace: a write since the first read
    // makes the offsets stale.
    let (latest, identity, target) = match parent.read_document_with_identity(file_name) {
        Ok(read) => read,
        Err(error) => {
            abandon();
            return Err(error);
        }
    };
    drop(target);
    if latest.version.fingerprint != expected_fingerprint {
        abandon();
        return Ok(DiskReplace::Stale);
    }
    let replaced = parent
        .ensure_entry_is(file_name, identity)
        .and_then(|()| parent.ensure_named_by(workspace, parent_path))
        .and_then(|()| parent.replace(temp_name, file_name));
    if let Err(error) = replaced {
        abandon();
        return Err(error);
    }
    parent.sync()?;
    Ok(DiskReplace::Written {
        fingerprint: fingerprint(materialized.as_bytes()),
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
