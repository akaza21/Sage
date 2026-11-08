//! Permanent deletion of an archived Chat, through the same purge that
//! removes temporary side chats.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    acp::service::ChatRuntimeRelease,
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

/// Whether the Chat belongs to that Project and is archived: attached to no
/// tab of any Project.
pub(crate) fn is_archived_chat(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> AppResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id=?2)
           AND NOT EXISTS(SELECT 1 FROM chat_tabs WHERE chat_id=?2)
           AND NOT EXISTS(SELECT 1 FROM workbench_tabs WHERE project_id=?1 AND id=?2)",
        params![project, chat],
        |row| row.get(0),
    )?)
}

/// Whether one of the Chat's Turns has not reached a terminal state.
pub(super) fn has_running_turn(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> AppResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns WHERE project_id=?1 AND chat_id=?2 AND terminal_at IS NULL)",
        params![project, chat],
        |row| row.get(0),
    )?)
}

/// Whether the Chat holds prompts queued to send after its current Turn.
pub(super) fn has_queued_prompts(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> AppResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_queued_prompts WHERE project_id=?1 AND chat_id=?2)",
        params![project, chat],
        |row| row.get(0),
    )?)
}

/// Admits only an archived Chat of that Project with no running Turn, no
/// live runtime and no fork descending from it. The delete command releases
/// an idle runtime before this runs.
fn admit_archived(connection: &Connection, project: &str, chat: &str) -> AppResult<()> {
    let state = connection
        .query_row(
            "SELECT
               EXISTS(SELECT 1 FROM chat_tabs WHERE chat_id=?2)
                 OR EXISTS(SELECT 1 FROM workbench_tabs WHERE project_id=?1 AND id=?2),
               EXISTS(SELECT 1 FROM turns
                      WHERE project_id=?1 AND chat_id=?2 AND terminal_at IS NULL),
               EXISTS(SELECT 1 FROM session_runtime_epochs
                      WHERE project_id=?1 AND chat_id=?2
                        AND state IN ('starting','running','terminating')),
               EXISTS(SELECT 1 FROM chat_lineage
                      WHERE parent_project_id=?1 AND parent_chat_id=?2)
             FROM chats WHERE project_id=?1 AND id=?2",
            params![project, chat],
            |row| Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?]),
        )
        .optional()?;
    let refusal = match state {
        None => "Chat does not belong to that Project".into(),
        Some([true, ..]) => "Only an archived Chat can be deleted".into(),
        Some([_, true, ..]) => ChatRuntimeRelease::TurnRunning.message("deleting"),
        Some([_, _, true, _]) => ChatRuntimeRelease::Unsettled.message("deleting"),
        Some([_, _, _, true]) => "Delete the Chats forked from this Chat first".into(),
        Some([false, false, false, false]) => return Ok(()),
    };
    Err(AppError::InvalidRequest(refusal))
}

#[cfg(test)]
#[path = "delete_tests.rs"]
mod tests;
