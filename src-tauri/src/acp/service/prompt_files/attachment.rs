//! Staged batches live only as long as the chat tab they were staged in.
use super::FileStages;

/// Files kept for Retry are released once their turn completed, since only
/// a failed or interrupted turn offers Retry. Checked with the tab sweep.
fn release_settled(stages: &std::sync::Mutex<FileStages>, database: &crate::persistence::Database) {
    use rusqlite::OptionalExtension;
    let held = stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .held_sent();
    let settled: Vec<_> = held
        .into_iter()
        .filter(|(project, chat, intent)| {
            database
                .connection()
                .query_row(
                    "SELECT 1 FROM turns WHERE project_id=?1 AND chat_id=?2 AND client_intent_id=?3 AND state='completed'",
                    rusqlite::params![project, chat, intent],
                    |_| Ok(()),
                )
                .optional()
                .is_ok_and(|row| row.is_some())
        })
        .collect();
    let mut stages = stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (project, chat, intent) in settled {
        stages.settle(&project, &chat, &intent);
    }
}

/// The one ownership rule for staged batches: a batch lives only while the
/// chat tab attachment it was staged in still exists. Any path that closes a
/// tab, deletes, moves or archives a chat, or saves a layout without the tab
/// ends that attachment; this drops the batch so it no longer counts against
/// the owner and memory caps and is never sent from a reopened tab. Attachment
/// commands and sends run it before they read stages, and layout persistence
/// runs it after a commit so closed tabs release their memory promptly.
pub(crate) fn release_detached(
    stages: &std::sync::Mutex<FileStages>,
    database: &crate::persistence::Database,
) {
    release_settled(stages, database);
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

/// The client intent of a turn in this chat and how many resources its
/// prompt carried, from the count persisted with the prompt (never bytes).
pub(crate) fn retry_origin(
    database: &crate::persistence::Database,
    project: &str,
    chat: &str,
    turn: &str,
) -> rusqlite::Result<Option<super::RetryOrigin>> {
    use rusqlite::OptionalExtension;
    let row: Option<(String, Option<String>)> = database
        .connection()
        .query_row(
            "SELECT t.client_intent_id, CASE WHEN i.payload_bytes<=262144 THEN i.payload_json END
             FROM turns t LEFT JOIN items i ON i.turn_id=t.id AND i.ordinal=0 AND i.kind='text'
             WHERE t.id=?1 AND t.project_id=?2 AND t.chat_id=?3",
            rusqlite::params![turn, project, chat],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(client_intent_id, payload)| super::RetryOrigin {
        client_intent_id,
        attachments: payload.as_deref().map_or(0, attachment_count),
    }))
}

/// The count a user prompt's payload records; an unreadable one counts as
/// carrying files, so Retry is refused rather than sent without them.
fn attachment_count(payload: &str) -> usize {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return usize::MAX;
    };
    let payload = value.get("payload").unwrap_or(&value);
    payload
        .get("attachments")
        .map_or(0, |count| count.as_u64().map_or(usize::MAX, |n| n as usize))
}

/// The `attached_at` of the chat's current tab, or `None` when it has none.
pub(crate) fn attachment(
    database: &crate::persistence::Database,
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
