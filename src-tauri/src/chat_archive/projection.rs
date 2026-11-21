use super::*;
use sage_domain::chat::ArchivedChatSummary;

const MAX_ARCHIVED_CHAT_BYTES: usize = MAX_ARCHIVED_CHATS * (512 + 1024);

pub(crate) use archived_chats_in as listed_archived_chats_in;

pub(crate) fn archived_chats_in(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<Vec<ArchivedChatSummary>> {
    let listed = crate::chat_lifecycle::visibility::listed_chat();
    let mut statement = connection.prepare(&format!(
        "SELECT c.id,c.title,{listed}
         FROM chats c
         WHERE c.project_id=?1 AND {listed}
           AND c.id NOT IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1)
           AND NOT EXISTS(
             SELECT 1 FROM chat_tabs a
             WHERE a.project_id=c.project_id AND a.chat_id=c.id
           )
         ORDER BY c.ordinal,c.id
         LIMIT ?2"
    )).db()?;
    let rows = statement
        .query_map(params![project_id, MAX_ARCHIVED_CHATS as i64 + 1], |row| {
            Ok(ArchivedChatSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                chat_listed: Some(row.get(2)?),
            })
        })
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
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
