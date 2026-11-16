use std::collections::HashSet;

use rusqlite::{OptionalExtension, params};
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
use sage_db::error::SqliteResultExt;
use serde_json::json;

use crate::project_registry::ProjectRegistryStore;
use crate::session_events::SessionEventStore;
use crate::{
    chat_archive::archived_chats_in,
    chat_lifecycle::{attach_chat_tab_in, durable_chat_title_in},
    harnesses::HarnessCatalog,
    project_registry::MAX_PROJECTS,
    workbench_restore::{
        restore_layout, validate_layout_tab_repository, validate_resource_for_project,
        validate_root_membership, validate_tab_projection,
    },
};
use sage_db::database::Database;
use sage_domain::{
    chat::ProjectLayoutRequest,
    project::{BootstrapSnapshot, ProjectSwitchRequest},
    repository::ProjectRepositoryStatus,
};

use self::bootstrap::{
    active_project_in, empty_snapshot, load_chat_id, load_session, restore_harnesses,
};

pub(crate) mod bootstrap;
pub(crate) mod store;

#[cfg(test)]
use self::store::WorkbenchStore;

pub use crate::workbench_restore::MAX_RESTORED_TABS;

pub(crate) fn persist_layout_in(
    transaction: &rusqlite::Transaction<'_>,
    request: &ProjectLayoutRequest,
) -> AppResult<()> {
    validate_layout(transaction, request)?;
    crate::side_chat_storage::validate_layout(transaction, request)?;
    let now = now_millis();
    transaction
        .execute(
            "UPDATE workbench_tabs SET position=-position-1 WHERE project_id=?1",
            [&request.project_id],
        )
        .db()?;
    for tab in &request.tabs {
        let title = if tab.kind == "chat" {
            durable_chat_title_in(transaction, &request.project_id, &tab.id)?
        } else {
            tab.title.clone()
        };
        crate::workbench_diff_ref::validate_diff_ref_for_persist(&request.project_id, tab)?;
        let metadata = match (&tab.resource, &tab.repository, &tab.diff_ref) {
            (Some(resource), Some(repository), None) => {
                json!({ "resource": resource, "repository": repository })
            }
            (Some(resource), None, Some(diff_ref)) => {
                json!({ "resource": resource, "diffRef": diff_ref })
            }
            (Some(resource), None, None) => json!({ "resource": resource }),
            (None, None, None) => json!({}),
            _ => {
                return Err(AppError::InvalidRequest(
                    "snapshot Diff tabs require their exact resource and no Git repository.".into(),
                ));
            }
        };
        transaction.execute(
            "INSERT INTO workbench_tabs
             (id, project_id, kind, title, position, pinned, dirty, status,
              metadata_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
             ON CONFLICT(id) DO UPDATE SET
               kind = excluded.kind,
               title = excluded.title,
               position = excluded.position,
               pinned = excluded.pinned,
               dirty = excluded.dirty,
               status = excluded.status,
               metadata_json = CASE WHEN json_extract(workbench_tabs.metadata_json,'$.ephemeral')=1 THEN workbench_tabs.metadata_json ELSE excluded.metadata_json END,
               updated_at = excluded.updated_at",
            params![
                tab.id,
                request.project_id,
                tab.kind,
                title,
                tab.position,
                i64::from(tab.pinned),
                i64::from(tab.dirty),
                tab.status,
                metadata.to_string(),
                now
            ],
        ).db()?;
        if tab.kind == "chat" {
            attach_chat_tab_in(transaction, &request.project_id, &tab.id, now)?;
        } else {
            transaction
                .execute(
                    "DELETE FROM chat_tabs WHERE project_id=?1 AND tab_id=?2",
                    params![request.project_id, tab.id],
                )
                .db()?;
        }
    }
    let retained: HashSet<_> = request.tabs.iter().map(|tab| tab.id.as_str()).collect();
    let existing = {
        let mut statement = transaction
            .prepare(
                "SELECT id FROM workbench_tabs WHERE project_id = ?1
             ORDER BY id LIMIT ?2",
            )
            .db()?;
        let rows = statement
            .query_map(
                params![request.project_id, MAX_RESTORED_TABS as i64 + 1],
                |row| row.get::<_, String>(0),
            )
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        if rows.len() > MAX_RESTORED_TABS {
            return Err(AppError::ResourceLimit(format!(
                "persisted project exceeds its {MAX_RESTORED_TABS}-tab limit"
            )));
        }
        rows
    };
    for stale_id in existing
        .into_iter()
        .filter(|id| !retained.contains(id.as_str()))
    {
        transaction
            .execute(
                "DELETE FROM chat_tabs WHERE project_id=?1 AND tab_id=?2",
                params![request.project_id, stale_id],
            )
            .db()?;
        transaction
            .execute(
                "DELETE FROM workbench_tabs
             WHERE project_id = ?1 AND id = ?2",
                params![request.project_id, stale_id],
            )
            .db()?;
    }
    transaction
        .execute(
            "INSERT INTO project_layouts
         (project_id, active_tab_id, selected_root_id,
          selected_relative_path, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(project_id) DO UPDATE SET
           active_tab_id = excluded.active_tab_id,
           selected_root_id = excluded.selected_root_id,
           selected_relative_path = excluded.selected_relative_path,
           updated_at = excluded.updated_at",
            params![
                request.project_id,
                request.active_tab_id,
                request.selected_root_id,
                request
                    .selected_resource
                    .as_ref()
                    .map(|resource| resource.relative_path.as_str()),
                now
            ],
        )
        .db()?;
    Ok(())
}

pub(crate) fn validate_layout(
    transaction: &rusqlite::Transaction<'_>,
    request: &ProjectLayoutRequest,
) -> AppResult<()> {
    if request.tabs.len() > MAX_RESTORED_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project layout may contain at most {MAX_RESTORED_TABS} tabs"
        )));
    }
    validate_tab_projection(&request.tabs)?;
    let mut ids = HashSet::new();
    let mut positions = HashSet::new();
    for tab in &request.tabs {
        if tab.position < 0 || !ids.insert(tab.id.as_str()) || !positions.insert(tab.position) {
            return Err(AppError::InvalidRequest(
                "project layout contains duplicate tab identity or position".into(),
            ));
        }
        let existing_owner = transaction
            .query_row(
                "SELECT project_id FROM workbench_tabs WHERE id = ?1",
                [&tab.id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .db()?;
        if existing_owner
            .as_deref()
            .is_some_and(|owner| owner != request.project_id)
        {
            return Err(AppError::InvalidRequest(
                "tab identity already belongs to another project".into(),
            ));
        }
        if !matches!(
            tab.kind.as_str(),
            "chat"
                | "file"
                | "diff"
                | "terminal"
                | "search"
                | "problems"
                | "output"
                | "settings"
                | "browser"
        ) {
            return Err(AppError::InvalidRequest(
                "project layout contains an unsupported tab kind".into(),
            ));
        }
        if let Some(resource) = &tab.resource {
            if tab.kind != "file" && tab.kind != "diff" {
                return Err(AppError::InvalidRequest(
                    "only File and Diff tabs may carry a resource".into(),
                ));
            }
            validate_resource_for_project(transaction, &request.project_id, resource)?;
        } else if tab.kind == "file" || tab.kind == "diff" {
            return Err(AppError::InvalidRequest(
                "File and Diff tabs require a resource identity".into(),
            ));
        }
        validate_layout_tab_repository(
            transaction,
            &request.project_id,
            &tab.kind,
            tab.resource.as_ref(),
            tab.repository.as_ref(),
        )?;
        crate::workbench_diff_ref::validate_diff_ref_for_persist(&request.project_id, tab)?;
    }
    if request
        .active_tab_id
        .as_ref()
        .is_some_and(|id| !ids.contains(id.as_str()))
    {
        return Err(AppError::InvalidRequest(
            "active tab is absent from the persisted layout".into(),
        ));
    }
    if let Some(root_id) = &request.selected_root_id {
        validate_root_membership(transaction, &request.project_id, root_id)?;
    }
    if let Some(resource) = &request.selected_resource {
        validate_resource_for_project(transaction, &request.project_id, resource)?;
        if request.selected_root_id.as_deref() != Some(resource.root_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "selected resource and selected root do not match".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
