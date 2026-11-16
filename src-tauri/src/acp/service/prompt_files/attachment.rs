use super::FileStages;

/// A batch lives only while the chat tab attachment it was staged in still
/// exists. Attachment commands and sends run this before they read stages,
/// and layout persistence runs it after a commit so closed tabs release their
/// memory promptly.
pub(crate) fn release_detached(
    stages: &std::sync::Mutex<FileStages>,
    database: &sage_db::database::Database,
) {
    let owners = stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .owners();
    if owners.is_empty() {
        return;
    }
    let ended: Vec<_> = owners
        .into_iter()
        .filter(|(project, chat, attached_at)| {
            attachment(database, project, chat).is_ok_and(|live| live != Some(*attached_at))
        })
        .collect();
    let mut stages = stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (project, chat, attached_at) in ended {
        let key = (project, chat);
        if stages
            .batches
            .get(&key)
            .is_some_and(|b| b.attached_at == attached_at)
        {
            stages.batches.remove(&key);
        }
        if stages
            .sent
            .get(&key)
            .is_some_and(|s| s.attached_at == attached_at)
        {
            stages.sent.remove(&key);
        }
    }
}

pub(crate) fn retry_origin(
    database: &sage_db::database::Database,
    project: &str,
    chat: &str,
    turn: &str,
) -> rusqlite::Result<Option<super::RetryOrigin>> {
    use rusqlite::OptionalExtension;
    let row: Option<(String, Option<String>)> = database
        .connection()
        .query_row(
            // The items schema bounds a payload at the normalized item cap,
            // so the prompt is read whole.
            "SELECT t.client_intent_id, i.payload_json
             FROM turns t LEFT JOIN items i ON i.turn_id=t.id AND i.ordinal=0 AND i.kind='text'
             WHERE t.id=?1 AND t.project_id=?2 AND t.chat_id=?3",
            rusqlite::params![turn, project, chat],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(client_intent_id, payload)| super::RetryOrigin {
        client_intent_id,
        // A turn with no prompt item is unknown, never 0.
        attachments: payload.as_deref().and_then(attachment_count),
    }))
}

/// The count a user prompt's payload records. `None` when the payload or
/// its count is unreadable, so Retry is refused rather than sent without
/// files it may have carried.
fn attachment_count(payload: &str) -> Option<usize> {
    let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
    let payload = value.get("payload").unwrap_or(&value);
    // Builds from before the count was recorded never sent prompt files,
    // so a prompt without a count carried none.
    let Some(count) = payload.get("attachments") else {
        return Some(0);
    };
    usize::try_from(count.as_u64()?).ok()
}

/// The `attached_at` of the chat's current tab, or `None` when it has none.
pub(crate) fn attachment(
    database: &sage_db::database::Database,
    project: &str,
    chat: &str,
) -> rusqlite::Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    database
        .connection()
        .query_row(
            "SELECT attached_at FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            rusqlite::params![project, chat],
            |row| row.get(0),
        )
        .optional()
}
