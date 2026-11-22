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
