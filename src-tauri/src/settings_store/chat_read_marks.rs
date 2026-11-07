//! Which chat updates the user has already seen. The Agent rail marks a
//! finished chat Unread until it is viewed; the mark has to outlive a reload
//! or a restart, or every finished chat comes back Unread. A mark records the
//! chat's last update time at the moment it was viewed, so a later update
//! makes the chat Unread again. Marks are view state for one user, bounded to
//! the most recently read chats.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::SettingsStore;
use crate::{chat_lifecycle::validate_chat_identifier, error::AppResult};

/// One mark for every chat the rail previews across all projects, which is
/// also every chat the cross-project attention query considers
/// (chat_read_state.rs), so a mark is evicted only by reads of chats that
/// have since left the rail.
pub const MAX_CHAT_READ_MARKS: usize = (crate::project_registry::MAX_PROJECTS
    * crate::project_registry::MAX_AGENT_RAIL_CHATS_PER_PROJECT)
    as usize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatReadMark {
    pub chat_id: String,
    /// The chat's update time, in Unix milliseconds, that the user has seen.
    pub read_through: i64,
}

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS chat_read_marks (
           chat_id TEXT PRIMARY KEY,
           read_through INTEGER NOT NULL CHECK (read_through >= 0),
           -- Read order: the most recently read marks are kept.
           sequence INTEGER NOT NULL
         ) STRICT;",
    )?;
    Ok(())
}

impl SettingsStore {
    pub fn chat_read_marks(&self) -> AppResult<Vec<ChatReadMark>> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT chat_id, read_through FROM chat_read_marks
             ORDER BY sequence DESC LIMIT ?1",
        )?;
        let marks = statement
            .query_map([MAX_CHAT_READ_MARKS as i64], |row| {
                Ok(ChatReadMark {
                    chat_id: row.get(0)?,
                    read_through: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(marks)
    }

    /// Records that the chat was seen through `read_through`. A mark never
    /// moves back, so a stale write cannot make a seen update Unread again.
    pub fn mark_chat_read(&self, mark: &ChatReadMark) -> AppResult<ChatReadMark> {
        validate_chat_identifier("Chat ID", &mark.chat_id)?;
        if mark.read_through < 0 {
            return Err(crate::error::AppError::InvalidRequest(
                "chat read time is negative".into(),
            ));
        }
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO chat_read_marks(chat_id, read_through, sequence)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM chat_read_marks))
             ON CONFLICT(chat_id) DO UPDATE SET
               read_through = MAX(read_through, excluded.read_through),
               sequence = excluded.sequence",
            params![mark.chat_id, mark.read_through],
        )?;
        transaction.execute(
            "DELETE FROM chat_read_marks WHERE chat_id NOT IN (
               SELECT chat_id FROM chat_read_marks
               ORDER BY sequence DESC LIMIT ?1
             )",
            [MAX_CHAT_READ_MARKS as i64],
        )?;
        let read_through = transaction.query_row(
            "SELECT read_through FROM chat_read_marks WHERE chat_id = ?1",
            [&mark.chat_id],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(ChatReadMark {
            chat_id: mark.chat_id.clone(),
            read_through,
        })
    }

    /// Forgets the mark, so the chat's latest update shows as Unread.
    pub fn mark_chat_unread(&self, chat_id: &str) -> AppResult<()> {
        validate_chat_identifier("Chat ID", chat_id)?;
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .execute("DELETE FROM chat_read_marks WHERE chat_id = ?1", [chat_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(chat_id: &str, read_through: i64) -> ChatReadMark {
        ChatReadMark {
            chat_id: chat_id.into(),
            read_through,
        }
    }

    #[test]
    fn a_read_mark_survives_reopening_the_store() {
        let directory =
            std::env::temp_dir().join(format!("sage-chat-read-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        SettingsStore::open(&path)
            .unwrap()
            .mark_chat_read(&mark("chat-a", 1_700))
            .unwrap();
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .chat_read_marks()
                .unwrap(),
            vec![mark("chat-a", 1_700)]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_stale_read_never_moves_a_mark_back() {
        let store = SettingsStore::in_memory().unwrap();
        store.mark_chat_read(&mark("chat-a", 2_000)).unwrap();
        assert_eq!(
            store.mark_chat_read(&mark("chat-a", 1_000)).unwrap(),
            mark("chat-a", 2_000)
        );
    }

    #[test]
    fn marking_unread_forgets_the_mark() {
        let store = SettingsStore::in_memory().unwrap();
        store.mark_chat_read(&mark("chat-a", 2_000)).unwrap();
        store.mark_chat_unread("chat-a").unwrap();
        assert!(store.chat_read_marks().unwrap().is_empty());
    }

    #[test]
    fn only_the_most_recently_read_chats_keep_a_mark() {
        let store = SettingsStore::in_memory().unwrap();
        for index in 0..=MAX_CHAT_READ_MARKS {
            store
                .mark_chat_read(&mark(&format!("chat-{index}"), 1))
                .unwrap();
        }
        let marks = store.chat_read_marks().unwrap();
        assert_eq!(marks.len(), MAX_CHAT_READ_MARKS);
        assert!(!marks.iter().any(|mark| mark.chat_id == "chat-0"));
        assert_eq!(marks[0].chat_id, format!("chat-{MAX_CHAT_READ_MARKS}"));
    }

    #[test]
    fn rejects_an_unbounded_chat_identifier() {
        let store = SettingsStore::in_memory().unwrap();
        assert!(store.mark_chat_read(&mark("", 1)).is_err());
        assert!(store.mark_chat_read(&mark("chat-a", -1)).is_err());
    }
}
