//! Opens Sage's database at startup. `sage-db` sets up the connections; the
//! steps around it run here in the order startup has always used: migrate,
//! settle turns a crash left open, open the database, purge temporary Chats,
//! then reconcile image artifacts. Nothing else touches the database before
//! this returns.

use std::path::Path;

use sage_core::error::AppResult;
use sage_db::{
    database::{Database, DatabaseOptions},
    physical_storage::StorageLimits,
};

use crate::acp::migration;
use crate::side_chat_storage::SideChatStore;

pub(crate) fn open_database(path: &Path) -> AppResult<Database> {
    open_database_with_limits(path, StorageLimits::production())
}

fn open_database_with_limits(path: &Path, storage_limits: StorageLimits) -> AppResult<Database> {
    let (connection, live_path_anchor) = migration::open_or_migrate(path)?.into_parts();
    crate::turn_workspace::invalidate_active(&connection).map_err(|_| {
        sage_core::error::AppError::InvalidRequest("workspace summary recovery failed".into())
    })?;
    let database = Database::open(
        path,
        connection,
        live_path_anchor,
        DatabaseOptions {
            reader_busy_timeout: crate::chat_search::READER_BUSY_TIMEOUT,
            storage_limits,
        },
    )?;
    // Like the exit path: a failed purge never blocks startup; the
    // temporary Chats stay hidden and the purge retries next time.
    if let Err(error) = database.purge_side_chats() {
        tracing::warn!(?error, "temporary Chat cleanup will retry later");
    }
    if let Ok(root) = database.image_artifact_root() {
        crate::image_artifacts::reconcile_image_artifacts(&database.connection(), root)?;
    }
    Ok(database)
}

#[cfg(test)]
mod tests {
    use crate::project_registry::create::ProjectCreateStore;
    use rusqlite::Connection;

    use super::*;

    #[test]
    fn fresh_database_applies_all_migrations_without_seeding_a_fake_project() {
        let database = open_database(Path::new(":memory:")).unwrap();
        let connection = database.connection();
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let project_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            migration_count,
            i64::from(crate::acp::migration::CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(project_count, 0);
    }

    #[test]
    fn editor_wal_limits_are_applied_to_the_live_connection() {
        let database = open_database(Path::new(":memory:")).unwrap();
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

    #[test]
    fn a_pinned_reader_keeps_seeing_the_store_as_it_was_when_pinned() {
        let directory =
            std::env::temp_dir().join(format!("sage-pinned-reader-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = open_database(&directory.join("sage.db")).unwrap();
        let projects = |connection: &Connection| -> i64 {
            connection
                .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
                .unwrap()
        };
        let reader = database.pinned_reader().unwrap().unwrap();

        database.create_project_from_root(&directory).unwrap();

        assert_eq!(projects(&database.connection()), 1);
        assert_eq!(projects(&reader), 0);
        drop(reader);
        assert!(
            open_database(Path::new(":memory:"))
                .unwrap()
                .pinned_reader()
                .unwrap()
                .is_none()
        );
        drop(database);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn wal_observation_reports_a_below_target_disk_database_without_checkpointing() {
        let directory =
            std::env::temp_dir().join(format!("sage-wal-observation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = open_database_with_limits(
            &directory.join("sage.db"),
            StorageLimits::fixture(4 * 1024 * 1024, 16 * 1024 * 1024, 32 * 1024 * 1024),
        )
        .unwrap();

        let receipt = database.converge_wal_for_observation().unwrap();

        assert!(!receipt.checkpointed());
        assert!(receipt.wal_bytes() < receipt.checkpoint_target_bytes());
        drop(database);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn wal_observation_truncates_once_when_the_physical_wal_reaches_the_target() {
        let directory =
            std::env::temp_dir().join(format!("sage-wal-observation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = open_database_with_limits(
            &directory.join("sage.db"),
            StorageLimits::fixture(4 * 1024 * 1024, 1, 32 * 1024 * 1024),
        )
        .unwrap();
        {
            let connection = database.write_connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE wal_observation_truncate(id INTEGER PRIMARY KEY);
                     INSERT INTO wal_observation_truncate DEFAULT VALUES;",
                )
                .unwrap();
        }

        let receipt = database.converge_wal_for_observation().unwrap();

        assert!(receipt.checkpointed());
        assert_eq!(receipt.wal_bytes(), 0);
        assert_eq!(receipt.checkpoint_target_bytes(), 1);
        drop(database);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn wal_observation_fails_closed_while_a_reader_pins_the_wal() {
        let directory =
            std::env::temp_dir().join(format!("sage-wal-observation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        let database = open_database_with_limits(
            &path,
            StorageLimits::fixture(4 * 1024 * 1024, 1, 32 * 1024 * 1024),
        )
        .unwrap();
        database.converge_wal_for_observation().unwrap();
        {
            let connection = database.write_connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE wal_observation_fixture(id INTEGER PRIMARY KEY, value TEXT);
                     INSERT INTO wal_observation_fixture(value) VALUES('base');",
                )
                .unwrap();
        }
        database.converge_wal_for_observation().unwrap();
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN").unwrap();
        let _: i64 = reader
            .query_row("SELECT COUNT(*) FROM wal_observation_fixture", [], |row| {
                row.get(0)
            })
            .unwrap();
        {
            let connection = database.write_connection().unwrap();
            connection
                .execute(
                    "INSERT INTO wal_observation_fixture(value) VALUES('pinned')",
                    [],
                )
                .unwrap();
        }
        database
            .connection()
            .pragma_update(None, "busy_timeout", 1)
            .unwrap();

        assert!(matches!(
            database.converge_wal_for_observation(),
            Err(sage_core::error::AppError::ResourceLimit(_))
        ));
        drop(reader);
        let receipt = database.converge_wal_for_observation().unwrap();
        assert!(receipt.checkpointed());
        assert_eq!(receipt.wal_bytes(), 0);
        drop(database);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn pinned_reader_forces_wal_hard_stop_and_release_converges_before_new_write() {
        let directory =
            std::env::temp_dir().join(format!("sage-wal-gate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        let database = open_database_with_limits(
            &path,
            StorageLimits::fixture(4 * 1024 * 1024, 16 * 1024, 32 * 1024),
        )
        .unwrap();
        {
            let connection = database.write_connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE wal_gate_fixture(id INTEGER PRIMARY KEY, payload BLOB);
                     INSERT INTO wal_gate_fixture(payload) VALUES(zeroblob(8192));
                     PRAGMA wal_checkpoint(TRUNCATE);",
                )
                .unwrap();
        }
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN").unwrap();
        let _: i64 = reader
            .query_row("SELECT COUNT(*) FROM wal_gate_fixture", [], |row| {
                row.get(0)
            })
            .unwrap();
        let mut committed = 0;
        for _ in 0..8 {
            let Ok(connection) = database.write_connection() else {
                break;
            };
            connection
                .execute(
                    "INSERT INTO wal_gate_fixture(payload) VALUES(zeroblob(8192))",
                    [],
                )
                .unwrap();
            committed += 1;
        }
        assert!(committed > 0);
        assert!(database.storage_recovery_required());
        assert!(matches!(
            database.write_connection(),
            Err(sage_core::error::AppError::ResourceLimit(_))
        ));
        drop(reader);
        {
            let connection = database.write_connection().unwrap();
            connection
                .execute(
                    "INSERT INTO wal_gate_fixture(payload) VALUES(zeroblob(1))",
                    [],
                )
                .unwrap();
        }
        assert!(!database.storage_recovery_required());
        drop(database);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn live_path_anchor_outlives_every_database_clone() {
        let directory =
            std::env::temp_dir().join(format!("sage-live-anchor-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        let moved = directory.join("moved.db");
        let database = open_database(&path).unwrap();
        let clone = database.clone();
        assert!(std::fs::remove_file(&path).is_err());
        assert!(std::fs::rename(&path, &moved).is_err());
        drop(database);
        assert!(std::fs::rename(&path, &moved).is_err());
        drop(clone);
        std::fs::rename(&path, &moved).unwrap();
        let _ = std::fs::remove_dir_all(directory);
    }
    #[test]
    fn cached_search_reader_refuses_concurrent_search_without_waiting() {
        let root =
            std::env::temp_dir().join(format!("sage-search-reader-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = open_database(&root.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let held = database.chat_search_reader().unwrap().unwrap();
        let shared = database.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = crate::chat_search::search_project_chats(
                &shared,
                &crate::chat_search::ProjectChatSearchRequest {
                    project_id: project.id,
                    project_generation: 0,
                    expected_transition_epoch: 0,
                    query: String::new(),
                },
            );
            send.send(result.is_err()).unwrap();
        });
        let refused = receive.recv_timeout(std::time::Duration::from_secs(1));
        drop(held);
        worker.join().unwrap();
        assert_eq!(refused, Ok(true));
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}
