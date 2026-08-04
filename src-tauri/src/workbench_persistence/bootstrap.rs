use rusqlite::{OptionalExtension, params};

use crate::{
    acp::intent::CapabilitySnapshotV1,
    error::{AppError, AppResult},
};

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
