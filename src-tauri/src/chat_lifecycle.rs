use rusqlite::{OptionalExtension, params};
mod auto_title;
mod create;
mod title_source;
pub(crate) use auto_title::{auto_title_first_prompt_in, title_line};
pub(crate) use create::insert_new_chat;
pub(crate) use title_source::{
    ChatTitleSource, SIDE_CHAT_ID_PREFIX, apply_harness_title_in, clear_harness_title_in,
    placeholder_chat_title,
};

use crate::{
    domain::RenameChatRequest,
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    workbench_persistence::persist_layout_in,
    workbench_restore::validate_tab_projection,
};

pub const MAX_CHAT_TITLE_BYTES: usize = 128;
const MAX_CHAT_ID_BYTES: usize = 512;

fn is_chat_title_trim_character(value: char) -> bool {
    matches!(
        value,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// Every stored Chat title starts with this; the UI shows what follows.
pub(crate) const CHAT_TITLE_PREFIX: &str = "Chat: ";

/// The title the user sees and named the Chat, without the stored prefix.
pub(crate) fn chat_display_title(stored: &str) -> &str {
    stored.strip_prefix(CHAT_TITLE_PREFIX).unwrap_or(stored)
}

pub(crate) fn canonical_chat_title(input: &str) -> AppResult<String> {
    let title = input.trim_matches(is_chat_title_trim_character);
    if title.is_empty() {
        return Err(AppError::InvalidRequest(
            "Chat title must not be empty after trimming".into(),
        ));
    }
    if title.len() > MAX_CHAT_TITLE_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "Chat title may contain at most {MAX_CHAT_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    if title.chars().any(char::is_control) {
        return Err(AppError::InvalidRequest(
            "Chat title must not contain control characters".into(),
        ));
    }
    Ok(format!("{CHAT_TITLE_PREFIX}{title}"))
}

pub(crate) fn validate_chat_identifier(label: &str, value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > MAX_CHAT_ID_BYTES || value.chars().any(char::is_control) {
        return Err(AppError::InvalidRequest(format!(
            "{label} is not a valid bounded Chat identifier"
        )));
    }
    Ok(())
}

pub(crate) fn durable_chat_title_in(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    chat_id: &str,
) -> AppResult<String> {
    transaction
        .query_row(
            "SELECT title FROM chats WHERE project_id=?1 AND id=?2",
            params![project_id, chat_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::InvalidRequest("Chat tab does not reference an existing durable Chat".into())
        })
}

pub(crate) fn attach_chat_tab_in(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    attached_at: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT OR IGNORE INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
         VALUES (?1,?2,?2,?3)",
        params![project_id, chat_id, attached_at],
    )?;
    let attached: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM chat_tabs
           WHERE project_id=?1 AND chat_id=?2 AND tab_id=?2
         )",
        params![project_id, chat_id],
        |row| row.get(0),
    )?;
    if !attached {
        return Err(AppError::InvalidRequest(
            "Chat tab identity conflicts with another attachment".into(),
        ));
    }
    Ok(())
}

impl Database {
    pub(crate) fn rename_chat(&self, request: &RenameChatRequest) -> AppResult<()> {
        crate::side_chat_storage::refuse_persistence(&self.connection(), &request.chat_id)?;
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        let title = canonical_chat_title(&request.title)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let active_project_id = transaction.query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )?;
        let source_is_active = active_project_id.as_deref() == Some(request.project_id.as_str());
        if source_is_active {
            if request.current_layout.project_id != request.project_id {
                return Err(AppError::InvalidRequest(
                    "Chat rename layout does not belong to the requested Project".into(),
                ));
            }
            validate_tab_projection(&request.current_layout.tabs)?;
            if !request
                .current_layout
                .tabs
                .iter()
                .any(|tab| tab.id == request.chat_id && tab.kind == "chat")
            {
                return Err(AppError::InvalidRequest(
                    "Chat rename target is not attached to the current layout".into(),
                ));
            }
            persist_layout_in(&transaction, &request.current_layout)?;
        }
        apply_chat_title(&transaction, request, &title)?;
        transaction.commit()?;
        Ok(())
    }
}

fn apply_chat_title(
    transaction: &rusqlite::Transaction<'_>,
    request: &RenameChatRequest,
    title: &str,
) -> AppResult<()> {
    let chat_exists: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM chats WHERE project_id=?1 AND id=?2
         )",
        params![request.project_id, request.chat_id],
        |row| row.get(0),
    )?;
    if !chat_exists {
        return Err(AppError::InvalidRequest(
            "Chat does not belong to that Project".into(),
        ));
    }
    let attached_tab_id = transaction.query_row(
        "SELECT tab_id FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
        params![request.project_id, request.chat_id],
        |row| row.get::<_, String>(0),
    )?;
    let now = now_millis();
    require_one(
        transaction.execute(
            "UPDATE chats SET title=?3,title_source=?4
             WHERE project_id=?1 AND id=?2",
            params![
                request.project_id,
                request.chat_id,
                title,
                ChatTitleSource::User.as_str()
            ],
        )?,
        "Chat disappeared during rename",
    )?;
    require_one(
        transaction.execute(
            "UPDATE workbench_tabs SET title=?4,updated_at=?5
             WHERE project_id=?1 AND id=?2 AND kind='chat'
               AND EXISTS(
                 SELECT 1 FROM chat_tabs
                 WHERE project_id=?1 AND chat_id=?3 AND tab_id=?2
               )",
            params![
                request.project_id,
                attached_tab_id,
                request.chat_id,
                title,
                now
            ],
        )?,
        "attached Chat tab disappeared during rename",
    )?;
    Ok(())
}

fn require_one(changed: usize, message: &str) -> AppResult<()> {
    if changed != 1 {
        return Err(AppError::InvalidRequest(message.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
