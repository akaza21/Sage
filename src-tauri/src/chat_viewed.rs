//! Whether the user has seen each chat's newest finished turn.
//!
//! A chat is read when no turn has finished since the user last viewed it.
//! The mark keys on turn completion times rather than the chat's
//! `updated_at`, which also moves for titles, moves and late projection
//! writes that are not new output for the user.

use rusqlite::params;

use crate::{
    chat_lifecycle::validate_chat_identifier,
    domain::MarkChatViewedRequest,
    error::{AppError, AppResult},
    persistence::Database,
};

/// SQL for a joined chat row `c`: whether its newest finished turn has been
/// viewed, or NULL when the row is not a chat. A chat with no finished turn
/// has nothing to read.
pub(crate) const CHAT_VIEWED_SQL: &str = "CASE WHEN c.id IS NULL THEN NULL ELSE
     COALESCE((SELECT MAX(t.terminal_at) FROM turns t
               WHERE t.project_id = c.project_id AND t.chat_id = c.id), 0)
       <= COALESCE(c.viewed_turn_at, 0) END";

impl Database {
    /// Marks the chat's newest finished turn as seen by the user.
    pub(crate) fn mark_chat_viewed(&self, request: &MarkChatViewedRequest) -> AppResult<()> {
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        let connection = self.write_connection()?;
        let changed = connection.execute(
            "UPDATE chats
             SET viewed_turn_at = (SELECT MAX(t.terminal_at) FROM turns t
                                   WHERE t.project_id = chats.project_id
                                     AND t.chat_id = chats.id)
             WHERE project_id = ?1
               AND id = (SELECT chat_id FROM chat_tabs WHERE project_id = ?1 AND tab_id = ?2)",
            params![request.project_id, request.chat_id],
        )?;
        if changed != 1 {
            return Err(AppError::InvalidRequest(
                "Chat is not attached to that Project".into(),
            ));
        }
        Ok(())
    }
}
