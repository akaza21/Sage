use rusqlite::params;

use crate::{
    chat_lifecycle::validate_chat_identifier,
    domain::PinChatRequest,
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
};

impl Database {
    pub(crate) fn pin_chat(&self, request: &PinChatRequest) -> AppResult<()> {
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let attached: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM workbench_tabs
             WHERE project_id=?1 AND id=?2 AND kind='chat'",
            params![request.project_id, request.chat_id],
            |row| row.get(0),
        )?;
        if attached != 1 {
            return Err(AppError::InvalidRequest(
                "Chat is not attached to that Project".into(),
            ));
        }
        let changed = transaction.execute(
            "UPDATE workbench_tabs
             SET pinned = CASE WHEN pinned = 0 THEN 1 ELSE 0 END, updated_at = ?3
             WHERE project_id=?1 AND id=?2 AND kind='chat'",
            params![request.project_id, request.chat_id, now_millis()],
        )?;
        if changed != 1 {
            return Err(AppError::InvalidRequest(
                "Chat pin could not be saved".into(),
            ));
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
