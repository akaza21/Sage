use rusqlite::{OptionalExtension, params};
use sage_db::error::SqliteResultExt;
use uuid::Uuid;
mod auto_title;
pub(crate) mod store;

#[cfg(test)]
use self::store::ChatLifecycleStore;
mod title_source;
pub(crate) use auto_title::{auto_title_first_prompt_in, title_line};
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
pub(crate) use title_source::{
    ChatTitleSource, SIDE_CHAT_ID_PREFIX, apply_harness_title_in, clear_harness_title_in,
    placeholder_chat_title,
};

use crate::{
    workbench_persistence::{MAX_RESTORED_TABS, persist_layout_in},
    workbench_restore::validate_tab_projection,
};
use sage_core::limits::LIMITS;
use sage_db::database::Database;
use sage_domain::chat::{ProjectLayoutRequest, RenameChatRequest};

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
        .optional()
        .db()?
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
    transaction
        .execute(
            "INSERT OR IGNORE INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
         VALUES (?1,?2,?2,?3)",
            params![project_id, chat_id, attached_at],
        )
        .db()?;
    let attached: bool = transaction
        .query_row(
            "SELECT EXISTS(
           SELECT 1 FROM chat_tabs
           WHERE project_id=?1 AND chat_id=?2 AND tab_id=?2
         )",
            params![project_id, chat_id],
            |row| row.get(0),
        )
        .db()?;
    if !attached {
        return Err(AppError::InvalidRequest(
            "Chat tab identity conflicts with another attachment".into(),
        ));
    }
    Ok(())
}

pub(crate) fn insert_new_chat(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source_is_active: bool,
    parent: Option<&str>,
) -> AppResult<String> {
    let chat_count: i64 = transaction
        .query_row(
            "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .db()?;
    if chat_count >= LIMITS.durable.chats_per_project as i64 {
        return Err(AppError::ResourceLimit(format!(
            "Project has reached its {}-Chat limit",
            LIMITS.durable.chats_per_project
        )));
    }
    let store_chat_count: i64 = transaction
        .query_row(
            "SELECT chat_count FROM acp_store_usage WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .db()?;
    if store_chat_count >= LIMITS.durable.chats_global as i64 {
        return Err(AppError::ResourceLimit(format!(
            "Sage has reached its {}-Chat limit",
            LIMITS.durable.chats_global
        )));
    }
    let ordinal: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(ordinal),-1)+1 FROM chats WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .db()?;
    let position: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(position),-1)+1 FROM workbench_tabs WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .db()?;
    let chat_id = if parent.is_some() {
        format!("{SIDE_CHAT_ID_PREFIX}{}", Uuid::new_v4())
    } else {
        Uuid::new_v4().to_string()
    };
    let title = placeholder_chat_title(&chat_id);
    let now = now_millis();
    transaction
        .execute(
            "INSERT INTO chats(
           id,project_id,ordinal,title,projection_revision,created_at,updated_at,title_source
         ) VALUES (?1,?2,?3,?4,0,?5,?5,?6)",
            params![
                chat_id,
                project_id,
                ordinal,
                title,
                now,
                ChatTitleSource::Default.as_str()
            ],
        )
        .db()?;
    transaction
        .execute(
            "INSERT INTO workbench_tabs(
           id,project_id,kind,title,position,pinned,dirty,status,
           metadata_json,created_at,updated_at
         ) VALUES (?1,?2,'chat',?3,?4,0,0,'idle','{}',?5,?5)",
            params![chat_id, project_id, title, position, now],
        )
        .db()?;
    transaction
        .execute(
            "INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
         VALUES (?1,?2,?2,?3)",
            params![project_id, chat_id, now],
        )
        .db()?;
    require_one(
        transaction
            .execute(
                "UPDATE project_layouts SET active_tab_id=?2,updated_at=?3 WHERE project_id=?1",
                params![project_id, chat_id, now],
            )
            .db()?,
        if source_is_active {
            "active Project layout disappeared during Chat creation"
        } else {
            "Project layout disappeared during Chat creation"
        },
    )?;
    require_one(
        transaction
            .execute(
                "UPDATE acp_project_usage
             SET chat_count=chat_count+1,revision=revision+1
             WHERE project_id=?1 AND chat_count=?2",
                params![project_id, chat_count],
            )
            .db()?,
        "Project Chat capacity changed during Chat creation",
    )?;
    require_one(
        transaction
            .execute(
                "UPDATE acp_store_usage
             SET chat_count=chat_count+1,revision=revision+1
             WHERE singleton=1 AND chat_count=?1",
                [store_chat_count],
            )
            .db()?,
        "global Chat capacity changed during Chat creation",
    )?;
    if let Some(parent) = parent {
        transaction
            .execute(
                "UPDATE workbench_tabs SET metadata_json=?2 WHERE id=?1",
                params![
                    chat_id,
                    serde_json::json!({"ephemeral":true,"parentChatId":parent}).to_string()
                ],
            )
            .db()?;
    }
    Ok(chat_id)
}

fn apply_chat_title(
    transaction: &rusqlite::Transaction<'_>,
    request: &RenameChatRequest,
    title: &str,
) -> AppResult<()> {
    let chat_exists: bool = transaction
        .query_row(
            "SELECT EXISTS(
           SELECT 1 FROM chats WHERE project_id=?1 AND id=?2
         )",
            params![request.project_id, request.chat_id],
            |row| row.get(0),
        )
        .db()?;
    if !chat_exists {
        return Err(AppError::InvalidRequest(
            "Chat does not belong to that Project".into(),
        ));
    }
    let attached_tab_id = transaction
        .query_row(
            "SELECT tab_id FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            params![request.project_id, request.chat_id],
            |row| row.get::<_, String>(0),
        )
        .db()?;
    let now = now_millis();
    require_one(
        transaction
            .execute(
                "UPDATE chats SET title=?3,title_source=?4
             WHERE project_id=?1 AND id=?2",
                params![
                    request.project_id,
                    request.chat_id,
                    title,
                    ChatTitleSource::User.as_str()
                ],
            )
            .db()?,
        "Chat disappeared during rename",
    )?;
    require_one(
        transaction
            .execute(
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
            )
            .db()?,
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
