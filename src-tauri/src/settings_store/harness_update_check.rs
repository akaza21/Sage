//! Settings › Harnesses › "Check npm for new harness versions". On by
//! default; while off, Sage starts no npm process.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;

use super::SettingsStore;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS harness_update_check (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
         ) STRICT;",
        )
        .db()?;
    Ok(())
}

impl SettingsStore {
    pub fn harness_update_check(&self) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(connection
            .query_row(
                "SELECT enabled FROM harness_update_check WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .db()?
            .unwrap_or(true))
    }

    pub fn set_harness_update_check(&self, enabled: bool) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection
            .execute(
                "INSERT INTO harness_update_check(singleton, enabled) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET enabled = excluded.enabled",
                params![enabled],
            )
            .db()?;
        Ok(enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsStore;

    #[test]
    fn checking_defaults_on_and_a_choice_survives_reopening_the_store() {
        let directory = std::env::temp_dir().join(format!(
            "sage-harness-update-check-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let store = SettingsStore::open(&path).unwrap();
        assert!(store.harness_update_check().unwrap());
        assert!(!store.set_harness_update_check(false).unwrap());
        assert!(!store.harness_update_check().unwrap());
        drop(store);
        assert!(
            !SettingsStore::open(&path)
                .unwrap()
                .harness_update_check()
                .unwrap()
        );
        std::fs::remove_dir_all(directory).ok();
    }
}
