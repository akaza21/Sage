use rusqlite::{OptionalExtension, params};

use crate::{
    chat_lifecycle::validate_chat_identifier,
    domain::MoveChatRequest,
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    workbench_persistence::persist_layout_in,
};

const MAX_CHATS_PER_PROJECT: i64 = 64;

mod legacy_welcome;

impl Database {
    pub(crate) fn move_chat(&self, request: &MoveChatRequest) -> AppResult<()> {
        crate::side_chat_storage::refuse_persistence(&self.connection(), &request.chat_id)?;
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        validate_chat_identifier("Target Project ID", &request.target_project_id)?;
        if request.project_id == request.target_project_id {
            return Err(AppError::InvalidRequest(
                "Chat already belongs to that Project".into(),
            ));
        }
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        crate::side_chat_storage::refuse_parent_removal(
            &transaction,
            &request.project_id,
            &request.chat_id,
        )?;
        let active: Option<String> = transaction.query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        let source_is_active = active.as_deref() == Some(request.project_id.as_str());
        if source_is_active {
            if request.current_layout.project_id != request.project_id {
                return Err(AppError::InvalidRequest(
                    "Chat layout does not belong to the source Project".into(),
                ));
            }
            persist_layout_in(&transaction, &request.current_layout)?;
        }
        let attached: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            params![request.project_id, request.chat_id],
            |row| row.get(0),
        )?;
        if attached != 1 {
            return Err(AppError::InvalidRequest(
                "Chat is not attached to the active Project".into(),
            ));
        }
        let target_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
            [&request.target_project_id],
            |row| row.get(0),
        )?;
        if !target_exists {
            return Err(AppError::InvalidRequest("unknown target Project".into()));
        }
        let sessions: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM agent_sessions WHERE project_id=?1 AND chat_id=?2",
            params![request.project_id, request.chat_id],
            |row| row.get(0),
        )?;
        let welcome =
            crate::session_events::legacy_welcome::turn_id(&transaction, &request.chat_id)?;
        if sessions > 0 && (sessions != 1 || welcome.is_none()) {
            return Err(AppError::InvalidRequest(
                "Chat with an agent session cannot move yet".into(),
            ));
        }
        let target_chats: i64 = transaction
            .query_row(
                "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
                [&request.target_project_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if target_chats >= MAX_CHATS_PER_PROJECT {
            return Err(AppError::ResourceLimit(format!(
                "Target Project has reached its {MAX_CHATS_PER_PROJECT}-Chat limit"
            )));
        }
        let now = now_millis();
        let ordinal: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(ordinal),-1)+1 FROM chats WHERE project_id=?1",
            [&request.target_project_id],
            |row| row.get(0),
        )?;
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position),-1)+1 FROM workbench_tabs WHERE project_id=?1",
            [&request.target_project_id],
            |row| row.get(0),
        )?;
        transaction.execute_batch("PRAGMA defer_foreign_keys = ON")?;
        if let Some(turn) = welcome {
            legacy_welcome::move_in(&transaction, request, &turn)?;
        }
        transaction.execute(
            "UPDATE chats SET project_id=?2, ordinal=?3, updated_at=?4 WHERE project_id=?1 AND id=?5",
            params![
                request.project_id,
                request.target_project_id,
                ordinal,
                now,
                request.chat_id
            ],
        )?;
        transaction.execute(
            "UPDATE workbench_tabs SET project_id=?2, position=?3, updated_at=?4
             WHERE project_id=?1 AND id=?5",
            params![
                request.project_id,
                request.target_project_id,
                position,
                now,
                request.chat_id
            ],
        )?;
        transaction.execute(
            "UPDATE chat_tabs SET project_id=?2 WHERE project_id=?1 AND chat_id=?3",
            params![
                request.project_id,
                request.target_project_id,
                request.chat_id
            ],
        )?;
        transaction.execute(
            "UPDATE acp_project_usage SET chat_count = chat_count - 1, revision = revision + 1
             WHERE project_id=?1 AND chat_count > 0",
            [&request.project_id],
        )?;
        let target_usage: Option<i64> = transaction
            .query_row(
                "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
                [&request.target_project_id],
                |row| row.get(0),
            )
            .optional()?;
        if target_usage.is_some() {
            transaction.execute(
                "UPDATE acp_project_usage SET chat_count = chat_count + 1, revision = revision + 1
                 WHERE project_id=?1",
                [&request.target_project_id],
            )?;
        }
        let source_next: Option<String> = transaction
            .query_row(
                "SELECT id FROM workbench_tabs
                 WHERE project_id=?1 AND kind='chat'
                 ORDER BY position, id LIMIT 1",
                [&request.project_id],
                |row| row.get(0),
            )
            .optional()?;
        transaction.execute(
            "UPDATE project_layouts SET active_tab_id=?2, updated_at=?3 WHERE project_id=?1",
            params![request.project_id, source_next, now],
        )?;
        transaction.execute(
            "UPDATE project_layouts SET active_tab_id=?2, updated_at=?3 WHERE project_id=?1",
            params![request.target_project_id, request.chat_id, now],
        )?;
        if source_is_active {
            transaction.execute(
                "UPDATE app_state SET active_project_id=?1, updated_at=?2 WHERE singleton=1",
                params![request.target_project_id, now],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
