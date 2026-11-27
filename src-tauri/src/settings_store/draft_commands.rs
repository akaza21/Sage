//! The last command list each harness reported, per project, kept so a new-chat
//! draft can offer the harness's whole catalog before its session exists. The
//! store holds the list as opaque JSON; `harnesses::draft_commands` shapes it.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::{AppError, AppResult};
use sage_db::error::SqliteResultExt;

use super::SettingsStore;

/// Lists kept across harnesses and projects; the least recently reported go first.
pub const MAX_DRAFT_CATALOGS: usize = 32;
/// One list's JSON. The live list itself is capped at 256 KiB of command text.
pub const MAX_DRAFT_CATALOG_BYTES: usize = 384 * 1024;
const MAX_KEY_BYTES: usize = 512;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS draft_command_catalogs (
           harness_id TEXT NOT NULL CHECK (length(CAST(harness_id AS BLOB)) BETWEEN 1 AND 512),
           -- '' is the list a harness reports outside any project.
           project_id TEXT NOT NULL CHECK (length(CAST(project_id AS BLOB)) <= 512),
           commands_json TEXT NOT NULL,
           sequence INTEGER NOT NULL,
           PRIMARY KEY (harness_id, project_id)
         ) STRICT;",
        )
        .db()?;
    Ok(())
}

fn admitted(harness_id: &str, project_id: &str) -> AppResult<()> {
    if harness_id.is_empty() || harness_id.len() > MAX_KEY_BYTES || project_id.len() > MAX_KEY_BYTES
    {
        return Err(AppError::InvalidRequest(
            "draft command catalog key is invalid".into(),
        ));
    }
    Ok(())
}

impl SettingsStore {
    /// Replaces the harness's list for `project_id` ('' for none) and evicts
    /// the least recently reported lists past [`MAX_DRAFT_CATALOGS`]. Returns
    /// whether the stored list changed.
    pub fn record_draft_commands(
        &self,
        harness_id: &str,
        project_id: &str,
        commands_json: &str,
    ) -> AppResult<bool> {
        admitted(harness_id, project_id)?;
        if commands_json.len() > MAX_DRAFT_CATALOG_BYTES {
            return Err(AppError::ResourceLimit(
                "draft command catalog exceeds its bound".into(),
            ));
        }
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction().db()?;
        let stored: Option<String> = transaction
            .query_row(
                "SELECT commands_json FROM draft_command_catalogs
             WHERE harness_id = ?1 AND project_id = ?2",
                params![harness_id, project_id],
                |row| row.get(0),
            )
            .optional()
            .db()?;
        transaction
            .execute(
                "INSERT INTO draft_command_catalogs(harness_id, project_id, commands_json, sequence)
             VALUES (?1, ?2, ?3,
               (SELECT COALESCE(MAX(sequence), 0) + 1 FROM draft_command_catalogs))
             ON CONFLICT(harness_id, project_id) DO UPDATE SET
               commands_json = excluded.commands_json, sequence = excluded.sequence",
                params![harness_id, project_id, commands_json],
            )
            .db()?;
        transaction
            .execute(
                "DELETE FROM draft_command_catalogs WHERE rowid IN (
               SELECT rowid FROM draft_command_catalogs ORDER BY sequence DESC
               LIMIT -1 OFFSET ?1)",
                [MAX_DRAFT_CATALOGS as i64],
            )
            .db()?;
        transaction.commit().db()?;
        Ok(stored.as_deref() != Some(commands_json))
    }

    /// The list reported for this project, else the harness's most recent one
    /// from anywhere: a command it names is checked against the live list on send.
    pub fn draft_commands(&self, harness_id: &str, project_id: &str) -> AppResult<Option<String>> {
        admitted(harness_id, project_id)?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection
            .query_row(
                "SELECT commands_json FROM draft_command_catalogs WHERE harness_id = ?1
             ORDER BY project_id = ?2 DESC, sequence DESC LIMIT 1",
                params![harness_id, project_id],
                |row| row.get(0),
            )
            .optional()
            .db()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(std::path::PathBuf);
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store() -> (Folder, SettingsStore) {
        let folder = std::env::temp_dir().join(format!("sage-drafts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let store = SettingsStore::open(&folder.join("settings.sqlite")).unwrap();
        (Folder(folder), store)
    }

    #[test]
    fn a_project_list_wins_and_another_projects_list_stands_in_for_it() {
        let (_folder, store) = store();
        assert_eq!(store.draft_commands("claude-code", "p1").unwrap(), None);
        assert!(
            store
                .record_draft_commands("claude-code", "", "[1]")
                .unwrap()
        );
        assert!(
            !store
                .record_draft_commands("claude-code", "", "[1]")
                .unwrap()
        );
        store
            .record_draft_commands("claude-code", "p2", "[2]")
            .unwrap();
        // No list for p1: the most recent one stands in.
        assert_eq!(
            store
                .draft_commands("claude-code", "p1")
                .unwrap()
                .as_deref(),
            Some("[2]")
        );
        store
            .record_draft_commands("claude-code", "p1", "[3]")
            .unwrap();
        store
            .record_draft_commands("claude-code", "", "[4]")
            .unwrap();
        assert_eq!(
            store
                .draft_commands("claude-code", "p1")
                .unwrap()
                .as_deref(),
            Some("[3]")
        );
        assert_eq!(
            store.draft_commands("claude-code", "").unwrap().as_deref(),
            Some("[4]")
        );
        assert_eq!(store.draft_commands("codex", "p1").unwrap(), None);
    }

    #[test]
    fn the_store_keeps_the_most_recent_lists_within_its_bounds() {
        let (_folder, store) = store();
        for index in 0..=MAX_DRAFT_CATALOGS {
            store
                .record_draft_commands("pi", &format!("p{index}"), "[]")
                .unwrap();
        }
        assert_eq!(
            store.draft_commands("pi", "p0").unwrap().as_deref(),
            Some("[]")
        );
        let count: i64 = store
            .0
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM draft_command_catalogs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, MAX_DRAFT_CATALOGS as i64);
        let oldest: i64 = store
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM draft_command_catalogs WHERE project_id = 'p0'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(oldest, 0);
        let oversized = "x".repeat(MAX_DRAFT_CATALOG_BYTES + 1);
        assert!(store.record_draft_commands("pi", "p1", &oversized).is_err());
    }
}
