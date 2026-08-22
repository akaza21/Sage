use rusqlite::{OptionalExtension, params};

use crate::{
    domain::{RenameProjectFileRequest, ResourceRef},
    error::{AppError, AppResult},
    persistence::now_millis,
    project_registry::RootRecord,
    workspace::Workspace,
};

use super::RenameKind;

pub(super) const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_FILE_RENAME_ID_BYTES: usize = 512;

pub(super) fn resolve_workspace(
    transaction: &rusqlite::Transaction<'_>,
    request: &RenameProjectFileRequest,
) -> AppResult<Workspace> {
    let active = transaction
        .query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if active.as_deref() != Some(request.project_id.as_str()) {
        return Err(AppError::InvalidRequest(
            "file rename Project is not active".into(),
        ));
    }
    let root = transaction
        .query_row(
            "SELECT id,project_id,absolute_path,ordinal
             FROM project_roots WHERE id=?1 AND project_id=?2",
            params![request.source.root_id, request.project_id],
            |row| {
                Ok(RootRecord {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    absolute_path: row.get::<_, String>(2)?.into(),
                    ordinal: row.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "file rename root does not belong to the active Project".into(),
            )
        })?;
    Workspace::open(&root.project_id, &root.id, &root.absolute_path)
}

pub(super) fn ensure_resources_closed(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source: &ResourceRef,
    target: &ResourceRef,
    kind: RenameKind,
) -> AppResult<()> {
    let tab_reference: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM workbench_tabs
           WHERE project_id=?1 AND kind IN ('file','diff')
             AND json_extract(metadata_json,'$.resource.rootId')=?2
             AND (
               json_extract(metadata_json,'$.resource.relativePath') IN (?3,?4)
               OR (
                 ?5=1
                 AND substr(json_extract(metadata_json,'$.resource.relativePath'),1,length(?3)+1)=?3 || '/'
               )
             )
         )",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            target.relative_path,
            (kind as u8)
        ],
        |row| row.get(0),
    )?;
    if tab_reference {
        return Err(AppError::InvalidRequest(
            "close File and Diff tabs for the source and target before renaming".into(),
        ));
    }
    let buffer_reference: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM editor_buffers
           WHERE project_id=?1 AND root_id=?2
             AND (
               relative_path IN (?3,?4)
               OR (?5=1 AND substr(relative_path,1,length(?3)+1)=?3 || '/')
             )
         )",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            target.relative_path,
            (kind as u8)
        ],
        |row| row.get(0),
    )?;
    if buffer_reference {
        return Err(AppError::InvalidRequest(
            "discard or save and close the source and target editor buffer before renaming".into(),
        ));
    }
    Ok(())
}

pub(super) fn ensure_request_resources_closed(
    request: &RenameProjectFileRequest,
    target: &ResourceRef,
    kind: RenameKind,
) -> AppResult<()> {
    let has_reference = request.current_layout.tabs.iter().any(|tab| {
        matches!(tab.kind.as_str(), "file" | "diff")
            && tab.resource.as_ref().is_some_and(|resource| {
                resource == &request.source
                    || resource == target
                    || (matches!(kind, RenameKind::Folder)
                        && resource.root_id == request.source.root_id
                        && resource
                            .relative_path
                            .strip_prefix(&request.source.relative_path)
                            .is_some_and(|suffix| suffix.starts_with('/')))
            })
    });
    if has_reference {
        return Err(AppError::InvalidRequest(
            "close File and Diff tabs for the source and target before renaming".into(),
        ));
    }
    Ok(())
}

pub(super) fn migrate_selection(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source: &ResourceRef,
    target: &ResourceRef,
) -> AppResult<()> {
    transaction.execute(
        "UPDATE project_layouts
         SET selected_relative_path=?4,updated_at=?5
         WHERE project_id=?1 AND selected_root_id=?2 AND selected_relative_path=?3",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            target.relative_path,
            now_millis()
        ],
    )?;
    Ok(())
}

pub(super) fn validate_request(request: &RenameProjectFileRequest) -> AppResult<()> {
    validate_request_id("Project ID", &request.project_id)?;
    validate_request_id("root ID", &request.source.root_id)?;
    if request.current_layout.project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "file rename layout does not belong to the requested Project".into(),
        ));
    }
    if request.project_generation > MAX_JAVASCRIPT_SAFE_INTEGER
        || request.expected_transition_epoch > MAX_JAVASCRIPT_SAFE_INTEGER
    {
        return Err(AppError::InvalidRequest(
            "file rename intent metadata is outside the safe integer range".into(),
        ));
    }
    Ok(())
}

fn validate_request_id(label: &str, value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > MAX_FILE_RENAME_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "{label} must be 1-{MAX_FILE_RENAME_ID_BYTES} UTF-8 bytes without control characters"
        )));
    }
    Ok(())
}
