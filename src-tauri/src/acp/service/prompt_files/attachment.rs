//! Staged batches live only as long as the chat tab they were staged in.
use super::FileStages;

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
