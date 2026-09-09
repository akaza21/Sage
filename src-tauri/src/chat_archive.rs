use std::collections::HashSet;

use rusqlite::{OptionalExtension, params};

use crate::{
    chat_lifecycle::validate_chat_identifier,
    domain::{ArchivedChatSummary, ChatLifecycleRequest, ProjectLayoutRequest, WorkbenchTab},
    error::{AppError, AppResult},
    persistence::Database,
    workbench_persistence::{MAX_RESTORED_TABS, persist_layout_in},
    workbench_restore::validate_tab_projection,
};

pub(crate) const MAX_ARCHIVED_CHATS: usize = 64;
const MAX_ARCHIVED_CHAT_BYTES: usize = MAX_ARCHIVED_CHATS * (512 + 1024);

pub(crate) fn archived_chats_in(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<Vec<ArchivedChatSummary>> {
    let mut statement = connection.prepare(
        "SELECT c.id,c.title
         FROM chats c
         WHERE c.project_id=?1
           AND NOT EXISTS(
             SELECT 1 FROM chat_tabs a
             WHERE a.project_id=c.project_id AND a.chat_id=c.id
           )
         ORDER BY c.ordinal,c.id
         LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![project_id, MAX_ARCHIVED_CHATS as i64 + 1], |row| {
            Ok(ArchivedChatSummary {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > MAX_ARCHIVED_CHATS {
        return Err(AppError::ResourceLimit(format!(
            "Project exceeds its {MAX_ARCHIVED_CHATS}-archived-Chat limit"
        )));
    }
    let mut bytes = 0_usize;
    for chat in &rows {
        validate_chat_identifier("Archived Chat ID", &chat.id)?;
        if chat.title.is_empty() || chat.title.len() > 1024 {
            return Err(AppError::InvalidRequest(
                "archived Chat title is invalid".into(),
            ));
        }
        bytes = bytes
            .checked_add(chat.id.len())
            .and_then(|value| value.checked_add(chat.title.len()))
            .ok_or_else(|| AppError::ResourceLimit("archived Chat metadata overflow".into()))?;
    }
    if bytes > MAX_ARCHIVED_CHAT_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "archived Chat metadata exceeds its {MAX_ARCHIVED_CHAT_BYTES}-byte limit"
        )));
    }
    Ok(rows)
}

impl Database {
    pub(crate) fn archive_chat(&self, request: &ChatLifecycleRequest) -> AppResult<()> {
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        if source_is_active(&transaction, &request.project_id)? {
            validate_request(request)?;
            let attached = require_exact_attached_chats(&transaction, &request.current_layout)?;
            if !attached.contains(request.chat_id.as_str()) {
                return Err(AppError::InvalidRequest(
                    "Chat is not attached to the active Project".into(),
                ));
            }
            if attached.len() <= 1 {
                return Err(AppError::InvalidRequest(
                    "the last attached Chat cannot be archived".into(),
                ));
            }
            let next_layout = archived_layout(&request.current_layout, &request.chat_id)?;
            persist_layout_in(&transaction, &next_layout)?;
        } else {
            archive_foreign_chat(&transaction, &request.project_id, &request.chat_id)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn restore_chat(&self, request: &ChatLifecycleRequest) -> AppResult<()> {
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        if source_is_active(&transaction, &request.project_id)? {
            validate_request(request)?;
            let attached = require_exact_attached_chats(&transaction, &request.current_layout)?;
            if attached.contains(request.chat_id.as_str()) {
                return Err(AppError::InvalidRequest(
                    "Chat is already attached to the active Project".into(),
                ));
            }
            if request.current_layout.tabs.len() >= MAX_RESTORED_TABS {
                return Err(AppError::ResourceLimit(format!(
                    "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
                )));
            }
            let title = chat_title_on_project(
                &transaction,
                &request.project_id,
                &request.chat_id,
                "Chat does not belong to the active Project",
            )?;
            let position = request
                .current_layout
                .tabs
                .iter()
                .map(|tab| tab.position)
                .max()
                .unwrap_or(-1)
                .checked_add(1)
                .ok_or_else(|| AppError::ResourceLimit("tab position is exhausted".into()))?;
            let mut next_layout = request.current_layout.clone();
            next_layout.tabs.push(WorkbenchTab {
                id: request.chat_id.clone(),
                kind: "chat".into(),
                title,
                position,
                pinned: false,
                dirty: false,
                status: Some("idle".into()),
                updated_at: None,
                resource: None,
                repository: None,
                diff_ref: None,
            });
            next_layout.active_tab_id = Some(request.chat_id.clone());
            validate_tab_projection(&next_layout.tabs)?;
            persist_layout_in(&transaction, &next_layout)?;
        } else {
            restore_foreign_chat(&transaction, &request.project_id, &request.chat_id)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn validate_request(request: &ChatLifecycleRequest) -> AppResult<()> {
    validate_chat_identifier("Project ID", &request.project_id)?;
    validate_chat_identifier("Chat ID", &request.chat_id)?;
    if request.current_layout.project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Chat lifecycle layout does not belong to the requested Project".into(),
        ));
    }
    validate_tab_projection(&request.current_layout.tabs)
}

fn source_is_active(transaction: &rusqlite::Transaction<'_>, project_id: &str) -> AppResult<bool> {
    let active = transaction.query_row(
        "SELECT active_project_id FROM app_state WHERE singleton=1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )?;
    Ok(active.as_deref() == Some(project_id))
}

fn chat_title_on_project(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    missing: &'static str,
) -> AppResult<String> {
    transaction
        .query_row(
            "SELECT title FROM chats WHERE project_id=?1 AND id=?2",
            params![project_id, chat_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::InvalidRequest(missing.into()))
}

fn restore_foreign_chat(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    chat_id: &str,
) -> AppResult<()> {
    let attached: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
        params![project_id, chat_id],
        |row| row.get(0),
    )?;
    if attached != 0 {
        return Err(AppError::InvalidRequest(
            "Chat is already attached to that Project".into(),
        ));
    }
    let tab_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM workbench_tabs WHERE project_id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    if tab_count >= MAX_RESTORED_TABS as i64 {
        return Err(AppError::ResourceLimit(format!(
            "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
        )));
    }
    let title = chat_title_on_project(
        transaction,
        project_id,
        chat_id,
        "Chat does not belong to that Project",
    )?;
    let now = crate::persistence::now_millis();
    let position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(position),-1)+1 FROM workbench_tabs WHERE project_id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO workbench_tabs
         (id, project_id, kind, title, position, pinned, dirty, status,
          metadata_json, created_at, updated_at)
         VALUES (?1, ?2, 'chat', ?3, ?4, 0, 0, 'idle', '{}', ?5, ?5)",
        params![chat_id, project_id, title, position, now],
    )?;
    crate::chat_lifecycle::attach_chat_tab_in(transaction, project_id, chat_id, now)?;
    transaction.execute(
        "UPDATE project_layouts SET active_tab_id=?2, updated_at=?3 WHERE project_id=?1",
        params![project_id, chat_id, now],
    )?;
    Ok(())
}

fn archive_foreign_chat(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    chat_id: &str,
) -> AppResult<()> {
    let attached: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM chat_tabs WHERE project_id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    if attached <= 1 {
        return Err(AppError::InvalidRequest(
            "the last attached Chat cannot be archived".into(),
        ));
    }
    let tab_id: Option<String> = transaction
        .query_row(
            "SELECT tab_id FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            params![project_id, chat_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(tab_id) = tab_id else {
        return Err(AppError::InvalidRequest(
            "Chat is not attached to that Project".into(),
        ));
    };
    let now = crate::persistence::now_millis();
    transaction.execute(
        "DELETE FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
        params![project_id, chat_id],
    )?;
    transaction.execute(
        "DELETE FROM workbench_tabs WHERE project_id=?1 AND id=?2 AND kind='chat'",
        params![project_id, tab_id],
    )?;
    let source_next: Option<String> = transaction
        .query_row(
            "SELECT id FROM workbench_tabs
             WHERE project_id=?1 AND kind='chat'
             ORDER BY position, id LIMIT 1",
            [project_id],
            |row| row.get(0),
        )
        .optional()?;
    transaction.execute(
        "UPDATE project_layouts SET active_tab_id=?2, updated_at=?3
         WHERE project_id=?1 AND active_tab_id=?4",
        params![project_id, source_next, now, chat_id],
    )?;
    Ok(())
}

fn require_exact_attached_chats(
    transaction: &rusqlite::Transaction<'_>,
    layout: &ProjectLayoutRequest,
) -> AppResult<HashSet<String>> {
    let requested = layout
        .tabs
        .iter()
        .filter(|tab| tab.kind == "chat")
        .map(|tab| tab.id.clone())
        .collect::<HashSet<_>>();
    let mut statement = transaction
        .prepare("SELECT chat_id FROM chat_tabs WHERE project_id=?1 ORDER BY chat_id LIMIT ?2")?;
    let rows = statement
        .query_map(
            params![layout.project_id, MAX_ARCHIVED_CHATS as i64 + 1],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > MAX_ARCHIVED_CHATS {
        return Err(AppError::ResourceLimit(format!(
            "Project exceeds its {MAX_ARCHIVED_CHATS}-attached-Chat limit"
        )));
    }
    let durable = rows.into_iter().collect::<HashSet<_>>();
    if requested != durable {
        return Err(AppError::InvalidRequest(
            "Chat attachments changed before the requested transition".into(),
        ));
    }
    Ok(durable)
}

fn archived_layout(
    current: &ProjectLayoutRequest,
    chat_id: &str,
) -> AppResult<ProjectLayoutRequest> {
    let mut chats = current
        .tabs
        .iter()
        .filter(|tab| tab.kind == "chat")
        .collect::<Vec<_>>();
    chats.sort_by_key(|tab| tab.position);
    let index = chats
        .iter()
        .position(|tab| tab.id == chat_id)
        .ok_or_else(|| AppError::InvalidRequest("archive target is not a Chat tab".into()))?;
    let next_chat_id = chats
        .get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|prior| chats.get(prior)))
        .map(|tab| tab.id.clone())
        .ok_or_else(|| {
            AppError::InvalidRequest("the last attached Chat cannot be archived".into())
        })?;
    let mut next = current.clone();
    next.tabs.retain(|tab| tab.id != chat_id);
    if next.active_tab_id.as_deref() == Some(chat_id) {
        next.active_tab_id = Some(next_chat_id);
    }
    Ok(next)
}

#[cfg(test)]
mod tests;
