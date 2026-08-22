use std::collections::HashSet;

use rusqlite::{OptionalExtension, params};
use serde_json::json;

use crate::{
    chat_archive::archived_chats_in,
    chat_lifecycle::{attach_chat_tab_in, durable_chat_title_in},
    domain::{
        BootstrapSnapshot, ProjectLayoutRequest, ProjectRepositoryStatus, ProjectSwitchRequest,
    },
    error::{AppError, AppResult},
    harnesses::{HarnessCatalog, SIMULATOR_ID},
    persistence::{Database, now_millis},
    project_registry::MAX_PROJECTS,
    workbench_restore::{
        restore_layout, validate_layout_tab_repository, validate_resource_for_project,
        validate_root_membership, validate_tab_projection,
    },
};

use self::bootstrap::{active_project_in, load_chat_id, load_session};

mod bootstrap;

pub use crate::workbench_restore::MAX_RESTORED_TABS;

impl Database {
    #[cfg(test)]
    pub fn bootstrap_snapshot(&self) -> AppResult<BootstrapSnapshot> {
        self.bootstrap_snapshot_with_catalog(&HarnessCatalog::default())
    }

    pub(crate) fn bootstrap_snapshot_with_catalog(
        &self,
        harnesses: &HarnessCatalog,
    ) -> AppResult<BootstrapSnapshot> {
        let active_project_id = self.repair_active_project()?;
        let projects = self.project_list()?;
        if projects.len() > MAX_PROJECTS as usize {
            return Err(AppError::ResourceLimit(format!(
                "project registry exceeds its {MAX_PROJECTS}-project limit"
            )));
        }
        let Some(project_id) = active_project_id else {
            return Ok(empty_snapshot(projects));
        };
        let project = self.project_summary(&project_id)?;
        let roots = self.root_records(&project_id)?;
        let (
            tabs,
            archived_chats,
            active_tab_id,
            selected_root_id,
            selected_resource,
            chat_id,
            session,
        ) = {
            let connection = self.connection();
            let restored = restore_layout(&connection, &project_id, &roots)?;
            let archived_chats = archived_chats_in(&connection, &project_id)?;
            let chat_id =
                load_chat_id(&connection, &project_id, restored.active_tab_id.as_deref())?;
            let session = load_session(&connection, &project_id, chat_id.as_deref())?;
            (
                restored.tabs,
                archived_chats,
                restored.active_tab_id,
                restored.selected_root_id,
                restored.selected_resource,
                chat_id,
                session,
            )
        };
        let messages = match &chat_id {
            Some(id) => self.messages_for_chat(id)?,
            None => Vec::new(),
        };
        let selected_harness_id = match &session {
            Some((_, binding_id, _)) => Some(
                harnesses
                    .harness_id_for_binding(binding_id)
                    .ok_or_else(|| {
                        AppError::InvalidRequest(
                            "active Chat head has no curated harness binding".into(),
                        )
                    })?,
            ),
            None => Some(SIMULATOR_ID.into()),
        };
        Ok(BootstrapSnapshot {
            transition_epoch: 0,
            projects,
            project: Some(project),
            tabs,
            archived_chats,
            active_tab_id,
            selected_root_id,
            selected_resource,
            harnesses: Vec::new(),
            session_id: session.as_ref().map(|value| value.0.clone()),
            selected_harness_id,
            selected_model_id: session.and_then(|value| value.2),
            messages,
            repositories: ProjectRepositoryStatus {
                roots: Vec::new(),
                truncated: false,
                elapsed_ms: 0,
            },
            turn_changes: Vec::new(),
            recovered_buffers: Vec::new(),
            recovery_notice: None,
        })
    }

    pub fn persist_project_layout(&self, request: &ProjectLayoutRequest) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let active = active_project_in(&transaction)?;
        if active.as_deref() != Some(request.project_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "layout does not belong to the active project".into(),
            ));
        }
        persist_layout_in(&transaction, request)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn switch_project(&self, request: &ProjectSwitchRequest) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let active = active_project_in(&transaction)?;
        if active.as_deref() != Some(request.current_layout.project_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "project changed before the requested transition".into(),
            ));
        }
        persist_layout_in(&transaction, &request.current_layout)?;
        let target_exists: bool = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM projects p
               WHERE p.id = ?1
                 AND EXISTS(
                   SELECT 1 FROM project_roots r WHERE r.project_id = p.id
                 )
             )",
            [&request.target_project_id],
            |row| row.get(0),
        )?;
        if !target_exists {
            return Err(AppError::InvalidRequest(
                "target project does not exist or has no roots".into(),
            ));
        }
        let now = now_millis();
        transaction.execute(
            "UPDATE app_state
             SET active_project_id = ?1, updated_at = ?2
             WHERE singleton = 1",
            params![request.target_project_id, now],
        )?;
        transaction.execute(
            "UPDATE projects
             SET last_opened_at = ?2, updated_at = ?2
             WHERE id = ?1",
            params![request.target_project_id, now],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn empty_snapshot(projects: Vec<crate::domain::ProjectListItem>) -> BootstrapSnapshot {
    BootstrapSnapshot {
        transition_epoch: 0,
        projects,
        project: None,
        tabs: Vec::new(),
        archived_chats: Vec::new(),
        active_tab_id: None,
        selected_root_id: None,
        selected_resource: None,
        harnesses: Vec::new(),
        session_id: None,
        selected_harness_id: None,
        selected_model_id: None,
        messages: Vec::new(),
        repositories: ProjectRepositoryStatus {
            roots: Vec::new(),
            truncated: false,
            elapsed_ms: 0,
        },
        turn_changes: Vec::new(),
        recovered_buffers: Vec::new(),
        recovery_notice: None,
    }
}

pub(crate) fn persist_layout_in(
    transaction: &rusqlite::Transaction<'_>,
    request: &ProjectLayoutRequest,
) -> AppResult<()> {
    validate_layout(transaction, request)?;
    let now = now_millis();
    for tab in &request.tabs {
        let title = if tab.kind == "chat" {
            durable_chat_title_in(transaction, &request.project_id, &tab.id)?
        } else {
            tab.title.clone()
        };
        let metadata = match (&tab.resource, &tab.repository) {
            (Some(resource), Some(repository)) => {
                json!({ "resource": resource, "repository": repository })
            }
            (Some(resource), None) => json!({ "resource": resource }),
            _ => json!({}),
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
               metadata_json = excluded.metadata_json,
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
        )?;
        if tab.kind == "chat" {
            attach_chat_tab_in(transaction, &request.project_id, &tab.id, now)?;
        } else {
            transaction.execute(
                "DELETE FROM chat_tabs WHERE project_id=?1 AND tab_id=?2",
                params![request.project_id, tab.id],
            )?;
        }
    }
    let retained: HashSet<_> = request.tabs.iter().map(|tab| tab.id.as_str()).collect();
    let existing = {
        let mut statement = transaction.prepare(
            "SELECT id FROM workbench_tabs WHERE project_id = ?1
             ORDER BY id LIMIT ?2",
        )?;
        let rows = statement
            .query_map(
                params![request.project_id, MAX_RESTORED_TABS as i64 + 1],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
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
        transaction.execute(
            "DELETE FROM chat_tabs WHERE project_id=?1 AND tab_id=?2",
            params![request.project_id, stale_id],
        )?;
        transaction.execute(
            "DELETE FROM workbench_tabs
             WHERE project_id = ?1 AND id = ?2",
            params![request.project_id, stale_id],
        )?;
    }
    transaction.execute(
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
    )?;
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
        if !ids.insert(tab.id.as_str()) || !positions.insert(tab.position) {
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
            .optional()?;
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
            "chat" | "file" | "diff" | "terminal" | "search" | "problems" | "output" | "settings"
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
