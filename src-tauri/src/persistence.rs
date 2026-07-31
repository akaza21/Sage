use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

use crate::{error::AppResult, project_migration};

const INITIAL_MIGRATION: &str = include_str!("../migrations/001_initial.sql");
const DIFF_TABS_MIGRATION: &str = include_str!("../migrations/002_diff_tabs.sql");
const EDITOR_BUFFERS_MIGRATION: &str = include_str!("../migrations/003_editor_buffers.sql");

#[derive(Debug, Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        connection.execute_batch(INITIAL_MIGRATION)?;
        apply_sql_migration(&connection, 2, DIFF_TABS_MIGRATION)?;
        apply_sql_migration(&connection, 3, EDITOR_BUFFERS_MIGRATION)?;
        project_migration::migrate(&mut connection)?;
        connection.pragma_update(None, "wal_autocheckpoint", 256)?;
        connection.pragma_update(None, "journal_size_limit", 8 * 1024 * 1024)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub(crate) fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn apply_sql_migration(connection: &Connection, version: i64, sql: &str) -> AppResult<()> {
    let applied: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM schema_migrations WHERE version = ?1
         )",
        [version],
        |row| row.get(0),
    )?;
    if !applied {
        connection.execute_batch(sql)?;
    }
    Ok(())
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_applies_all_migrations_without_seeding_a_fake_project() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let connection = database.connection();
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let project_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(migration_count, 4);
        assert_eq!(project_count, 0);
    }

    #[test]
    fn editor_wal_limits_are_applied_to_the_live_connection() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let connection = database.connection();
        let auto_checkpoint: i64 = connection
            .pragma_query_value(None, "wal_autocheckpoint", |row| row.get(0))
            .unwrap();
        let journal_size_limit: i64 = connection
            .pragma_query_value(None, "journal_size_limit", |row| row.get(0))
            .unwrap();
        assert_eq!(auto_checkpoint, 256);
        assert_eq!(journal_size_limit, 8 * 1024 * 1024);
    }
}
