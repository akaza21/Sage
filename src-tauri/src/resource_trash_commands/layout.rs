use rusqlite::params;
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};

use crate::workspace::resource_trash::TRASH_NAME;
use sage_domain::{file_operations::TrashProjectResourceRequest, resource::ResourceRef};

pub(super) fn same_or_descendant(candidate: &ResourceRef, source: &ResourceRef) -> bool {
    candidate.root_id == source.root_id
        && (candidate.relative_path == source.relative_path
            || candidate
                .relative_path
                .strip_prefix(&source.relative_path)
                .is_some_and(|suffix| suffix.starts_with('/')))
}

pub(super) fn ensure_resource_closed(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source: &ResourceRef,
) -> AppResult<()> {
    let tab_reference: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM workbench_tabs
           WHERE project_id=?1 AND kind IN ('file','diff')
             AND json_extract(metadata_json,'$.resource.rootId')=?2
             AND (
               json_extract(metadata_json,'$.resource.relativePath')=?3 OR
               substr(json_extract(metadata_json,'$.resource.relativePath'),1,length(?3)+1)=?3||'/'
             )
         )",
        params![project_id, source.root_id, source.relative_path],
        |row| row.get(0),
    )?;
    if tab_reference {
        return Err(AppError::InvalidRequest(format!(
            "close File and Diff tabs for the resource and its descendants before moving it to the {TRASH_NAME}"
        )));
    }
    let buffer_reference: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM editor_buffers
           WHERE project_id=?1 AND root_id=?2
             AND (relative_path=?3 OR substr(relative_path,1,length(?3)+1)=?3||'/')
         )",
        params![project_id, source.root_id, source.relative_path],
        |row| row.get(0),
    )?;
    if buffer_reference {
        return Err(AppError::InvalidRequest(format!(
            "save or discard and close editor buffers for the resource and its descendants before moving it to the {TRASH_NAME}"
        )));
    }
    Ok(())
}

pub(super) fn ensure_request_resource_closed(
    request: &TrashProjectResourceRequest,
) -> AppResult<()> {
    let has_reference = request.current_layout.tabs.iter().any(|tab| {
        matches!(tab.kind.as_str(), "file" | "diff")
            && tab
                .resource
                .as_ref()
                .is_some_and(|resource| same_or_descendant(resource, &request.source))
    });
    if has_reference {
        return Err(AppError::InvalidRequest(format!(
            "close File and Diff tabs for the resource and its descendants before moving it to the {TRASH_NAME}"
        )));
    }
    Ok(())
}

pub(super) fn migrate_selection(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source: &ResourceRef,
    parent: &ResourceRef,
) -> AppResult<()> {
    transaction.execute(
        "UPDATE project_layouts
         SET selected_relative_path=?4,updated_at=?5
         WHERE project_id=?1 AND selected_root_id=?2
           AND (selected_relative_path=?3 OR substr(selected_relative_path,1,length(?3)+1)=?3||'/')",
        params![
            project_id,
            source.root_id,
            source.relative_path,
            parent.relative_path,
            now_millis()
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendant_matching_is_segment_aware_and_root_exact() {
        let source = ResourceRef {
            root_id: "root-a".into(),
            relative_path: "src/app".into(),
        };
        for path in ["src/app", "src/app/main.rs"] {
            assert!(same_or_descendant(
                &ResourceRef {
                    root_id: "root-a".into(),
                    relative_path: path.into(),
                },
                &source
            ));
        }
        for (root, path) in [("root-a", "src/apple.rs"), ("root-b", "src/app/main.rs")] {
            assert!(!same_or_descendant(
                &ResourceRef {
                    root_id: root.into(),
                    relative_path: path.into(),
                },
                &source
            ));
        }
    }
}
