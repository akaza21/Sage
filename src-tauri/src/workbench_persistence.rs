use std::collections::HashSet;

use rusqlite::{OptionalExtension, params};
use serde_json::json;

use crate::{
    domain::{
        BootstrapSnapshot, ProjectLayoutRequest, ProjectRepositoryStatus, ProjectSwitchRequest,
    },
    error::{AppError, AppResult},
    harnesses::SIMULATOR_ID,
    persistence::{Database, now_millis},
    project_registry::MAX_PROJECTS,
    workbench_restore::{restore_layout, validate_resource_for_project, validate_tab_projection},
};

pub use crate::workbench_restore::MAX_RESTORED_TABS;

impl Database {
    pub fn bootstrap_snapshot(&self) -> AppResult<BootstrapSnapshot> {
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
        let (tabs, active_tab_id, selected_root_id, selected_resource, session) = {
            let connection = self.connection();
            let restored = restore_layout(&connection, &project_id, &roots)?;
            let session =
                load_session(&connection, &project_id, restored.active_tab_id.as_deref())?;
            (
                restored.tabs,
                restored.active_tab_id,
                restored.selected_root_id,
                restored.selected_resource,
                session,
            )
        };
        let messages = match &session {
            Some((id, _, _)) => self.messages(id)?,
            None => Vec::new(),
        };
        Ok(BootstrapSnapshot {
            projects,
            project: Some(project),
            tabs,
            active_tab_id,
            selected_root_id,
            selected_resource,
            harnesses: Vec::new(),
            session_id: session.as_ref().map(|value| value.0.clone()),
            selected_harness_id: session
                .as_ref()
                .map(|value| value.1.clone())
                .or_else(|| Some(SIMULATOR_ID.into())),
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
        let mut connection = self.connection();
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
        let mut connection = self.connection();
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
        projects,
        project: None,
        tabs: Vec::new(),
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

fn persist_layout_in(
    transaction: &rusqlite::Transaction<'_>,
    request: &ProjectLayoutRequest,
) -> AppResult<()> {
    validate_layout(transaction, request)?;
    let now = now_millis();
    for tab in &request.tabs {
        let metadata = tab
            .resource
            .as_ref()
            .map_or_else(|| json!({}), |resource| json!({ "resource": resource }));
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
                tab.title,
                tab.position,
                i64::from(tab.pinned),
                i64::from(tab.dirty),
                tab.status,
                metadata.to_string(),
                now
            ],
        )?;
    }
    let retained: HashSet<_> = request.tabs.iter().map(|tab| tab.id.as_str()).collect();
    let existing = {
        let mut statement =
            transaction.prepare("SELECT id FROM workbench_tabs WHERE project_id = ?1")?;
        statement
            .query_map([&request.project_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for stale_id in existing
        .into_iter()
        .filter(|id| !retained.contains(id.as_str()))
    {
        transaction.execute(
            "DELETE FROM workbench_tabs WHERE project_id = ?1 AND id = ?2",
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

fn validate_layout(
    transaction: &rusqlite::Transaction<'_>,
    request: &ProjectLayoutRequest,
) -> AppResult<()> {
    if request.tabs.is_empty() || request.tabs.len() > MAX_RESTORED_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project layout must contain 1..={MAX_RESTORED_TABS} tabs"
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

fn validate_root_membership(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    root_id: &str,
) -> AppResult<()> {
    let matches: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM project_roots WHERE id = ?1 AND project_id = ?2
         )",
        params![root_id, project_id],
        |row| row.get(0),
    )?;
    if !matches {
        return Err(AppError::InvalidRequest(
            "layout resource root does not belong to its project".into(),
        ));
    }
    Ok(())
}

fn active_project_in(transaction: &rusqlite::Transaction<'_>) -> AppResult<Option<String>> {
    Ok(transaction
        .query_row(
            "SELECT active_project_id FROM app_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}

fn load_session(
    connection: &rusqlite::Connection,
    project_id: &str,
    active_tab_id: Option<&str>,
) -> AppResult<Option<(String, String, Option<String>)>> {
    Ok(connection
        .query_row(
            "SELECT id, harness_id, model_id
             FROM agent_sessions
             WHERE project_id = ?1
             ORDER BY CASE WHEN chat_tab_id = ?2 THEN 0 ELSE 1 END, updated_at DESC
             LIMIT 1",
            params![project_id, active_tab_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}
