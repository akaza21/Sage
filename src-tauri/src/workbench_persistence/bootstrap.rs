use rusqlite::{OptionalExtension, params};
use sage_core::error::{AppError, AppResult};

use crate::{
    harnesses::HarnessCatalog, persistence::Database, project_registry::MAX_PROJECTS,
    workbench_restore::MAX_RESTORED_TABS,
};
use sage_acp_protocol::intent::CapabilitySnapshotV1;
use sage_domain::{
    chat::WorkbenchTab,
    harness::HarnessDefinition,
    project::{BootstrapSnapshot, ProjectListItem},
    repository::ProjectRepositoryStatus,
};

pub(super) fn restore_harnesses(
    connection: &rusqlite::Connection,
    project_id: &str,
    tabs: &[WorkbenchTab],
    catalog: &HarnessCatalog,
) -> AppResult<Vec<HarnessDefinition>> {
    let mut definitions = catalog.definitions();
    // The restored layout is capped at MAX_RESTORED_TABS. Resolve at most one
    // binding per Chat, preferring its head over its latest historical session.
    let mut statement = connection.prepare(
        "SELECT s.binding_id FROM chat_tabs t
         JOIN agent_sessions s ON s.project_id=t.project_id AND s.chat_id=t.chat_id
         LEFT JOIN chat_session_heads h ON h.project_id=s.project_id AND h.chat_id=s.chat_id
         WHERE t.project_id=?1 AND t.tab_id=?2 AND s.legacy=0
         ORDER BY CASE WHEN s.id=h.session_id THEN 0 ELSE 1 END, s.ordinal DESC
         LIMIT 1",
    )?;
    for tab in tabs.iter().filter(|tab| tab.kind == "chat") {
        let binding: Option<String> = statement
            .query_row(params![project_id, tab.id], |row| row.get(0))
            .optional()?;
        if let Some(binding) = binding {
            catalog.restore_binding(&binding, &mut definitions);
        }
    }
    Ok(definitions)
}

impl Database {
    pub(crate) fn next_browser_tab_number(&self) -> AppResult<u64> {
        let limit = MAX_PROJECTS * MAX_RESTORED_TABS as i64;
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT CASE WHEN length(CAST(id AS BLOB)) > 28 THEN '' ELSE id END
             FROM workbench_tabs WHERE kind='browser' LIMIT ?1",
        )?;
        let mut rows = statement.query([limit + 1])?;
        let mut count = 0_i64;
        let mut maximum = 0_u64;
        while let Some(row) = rows.next()? {
            count += 1;
            if count > limit {
                return Err(AppError::ResourceLimit(
                    "persisted browser tab count exceeds its limit".into(),
                ));
            }
            let id: String = row.get(0)?;
            let number = id
                .strip_prefix("browser-")
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0 && format!("browser-{value}") == id)
                .ok_or_else(|| {
                    AppError::InvalidRequest(
                        "persisted browser tab has a malformed identity".into(),
                    )
                })?;
            maximum = maximum.max(number);
        }
        maximum
            .checked_add(1)
            .ok_or_else(|| AppError::ResourceLimit("browser tab labels exhausted".into()))
    }
}

pub(super) fn empty_snapshot(projects: Vec<ProjectListItem>) -> BootstrapSnapshot {
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

pub(super) fn active_project_in(
    transaction: &rusqlite::Transaction<'_>,
) -> AppResult<Option<String>> {
    Ok(transaction
        .query_row(
            "SELECT active_project_id FROM app_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}

pub(super) fn load_session(
    connection: &rusqlite::Connection,
    project_id: &str,
    chat_id: Option<&str>,
) -> AppResult<Option<(String, String, Option<String>)>> {
    let Some(chat_id) = chat_id else {
        return Ok(None);
    };
    let row = connection
        .query_row(
            "SELECT s.id,s.binding_id,s.model_id,s.state,s.legacy,s.external_session_id,
                    s.capabilities_json,s.projection_revision,c.projection_revision,
                    h.installed_revision
             FROM chat_session_heads h
             JOIN agent_sessions s
               ON s.project_id=h.project_id AND s.chat_id=h.chat_id AND s.id=h.session_id
             JOIN chats c ON c.project_id=h.project_id AND c.id=h.chat_id
             WHERE h.project_id=?1 AND h.chat_id=?2",
            params![project_id, chat_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        id,
        binding,
        model,
        state,
        legacy,
        external,
        capabilities,
        session_projection,
        chat_projection,
        installed_projection,
    )) = row
    else {
        return Ok(None);
    };
    let requires_external = matches!(
        state.as_str(),
        "ready" | "active" | "recovering" | "closing"
    );
    if legacy != 0
        || !matches!(
            state.as_str(),
            "created" | "starting" | "ready" | "active" | "recovering" | "closing"
        )
        || session_projection != chat_projection
        || installed_projection > chat_projection
        || requires_external && external.as_deref().is_none_or(str::is_empty)
        || serde_json::from_str::<CapabilitySnapshotV1>(&capabilities).is_err()
    {
        return Err(AppError::InvalidRequest(
            "active Chat head metadata is corrupt".into(),
        ));
    }
    Ok(Some((id, binding, model)))
}

pub(super) fn load_chat_id(
    connection: &rusqlite::Connection,
    project_id: &str,
    active_tab_id: Option<&str>,
) -> AppResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT c.chat_id
             FROM chat_tabs c
             JOIN workbench_tabs t ON t.project_id=c.project_id AND t.id=c.tab_id
             JOIN chats h ON h.project_id=c.project_id AND h.id=c.chat_id
             WHERE c.project_id=?1 AND t.kind='chat'
             ORDER BY CASE WHEN c.tab_id=?2 THEN 0 ELSE 1 END,
                      h.ordinal DESC,h.id DESC
             LIMIT 1",
            params![project_id, active_tab_id],
            |row| row.get(0),
        )
        .optional()?)
}
