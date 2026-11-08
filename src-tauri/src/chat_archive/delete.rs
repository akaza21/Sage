//! Permanent deletion of an archived Chat, through the same purge that
//! removes temporary side chats.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    chat_lifecycle::validate_chat_identifier,
    domain::ChatDeleteRequest,
    error::{AppError, AppResult},
    persistence::Database,
};

impl Database {
    /// Deletes an archived Chat with its history and releases its usage, so
    /// the Project can create a Chat in its place.
    pub(crate) fn delete_archived_chat(&self, request: &ChatDeleteRequest) -> AppResult<()> {
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        self.purge_chat(&request.project_id, &request.chat_id, |connection| {
            admit_archived(connection, &request.project_id, &request.chat_id)
        })
    }
}

/// Admits only an archived Chat of that Project with no live runtime, no
/// unfinished Turn and no fork descending from it.
fn admit_archived(connection: &Connection, project: &str, chat: &str) -> AppResult<()> {
    let state = connection
        .query_row(
            "SELECT
               EXISTS(SELECT 1 FROM chat_tabs WHERE chat_id=?2)
                 OR EXISTS(SELECT 1 FROM workbench_tabs WHERE project_id=?1 AND id=?2),
               EXISTS(SELECT 1 FROM session_runtime_epochs
                      WHERE project_id=?1 AND chat_id=?2
                        AND state IN ('starting','running','terminating'))
                 OR EXISTS(SELECT 1 FROM turns
                           WHERE project_id=?1 AND chat_id=?2 AND terminal_at IS NULL),
               EXISTS(SELECT 1 FROM chat_lineage
                      WHERE parent_project_id=?1 AND parent_chat_id=?2)
             FROM chats WHERE project_id=?1 AND id=?2",
            params![project, chat],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get(2)?)),
        )
        .optional()?;
    let refusal = match state {
        None => "Chat does not belong to that Project",
        Some((true, _, _)) => "Only an archived Chat can be deleted",
        Some((_, true, _)) => "Stop this Chat's agent before deleting it",
        Some((_, _, true)) => "Delete the Chats forked from this Chat first",
        Some((false, false, false)) => return Ok(()),
    };
    Err(AppError::InvalidRequest(refusal.into()))
}

#[cfg(test)]
#[path = "delete_tests.rs"]
mod tests;
