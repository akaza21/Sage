use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::{AppError, AppResult};

use crate::{
    acp::service::ChatRuntimeRelease, chat_lifecycle::validate_chat_identifier,
    domain::ChatDeleteRequest, persistence::Database,
};

impl Database {
    pub(crate) fn delete_archived_chat(&self, request: &ChatDeleteRequest) -> AppResult<()> {
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        self.purge_chat(&request.project_id, &request.chat_id, |connection| {
            admit_archived(connection, &request.project_id, &request.chat_id)
        })
    }
}

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
