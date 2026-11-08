use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

use crate::{acp::migration, error::AppResult};

mod physical_storage;

use physical_storage::{PhysicalStorage, StorageLimits};

#[cfg(any(test, all(windows, feature = "acp-fixture")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WalConvergenceReceipt {
    wal_bytes: u64,
    checkpoint_target_bytes: u64,
    checkpointed: bool,
}

#[cfg(any(test, all(windows, feature = "acp-fixture")))]
impl WalConvergenceReceipt {
    pub(crate) fn wal_bytes(self) -> u64 {
        self.wal_bytes
    }

    pub(crate) fn checkpoint_target_bytes(self) -> u64 {
        self.checkpoint_target_bytes
    }

    pub(crate) fn checkpointed(self) -> bool {
        self.checkpointed
    }

    #[cfg(all(test, feature = "acp-fixture", windows))]
    pub(crate) fn test_only(
        wal_bytes: u64,
        checkpoint_target_bytes: u64,
        checkpointed: bool,
    ) -> Self {
        Self {
            wal_bytes,
            checkpoint_target_bytes,
            checkpointed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
    search_reader: Option<Arc<Mutex<Connection>>>,
    physical_storage: Arc<PhysicalStorage>,
    _live_path_anchor: Arc<migration::LivePathAnchor>,
    image_artifact_root: Option<Arc<PathBuf>>,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        Self::open_with_limits(path, StorageLimits::production())
    }

    fn open_with_limits(path: &Path, limits: StorageLimits) -> AppResult<Self> {
        let (connection, live_path_anchor) = migration::open_or_migrate(path)?.into_parts();
        crate::turn_workspace::invalidate_active(&connection).map_err(|_| {
            crate::error::AppError::InvalidRequest("workspace summary recovery failed".into())
        })?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        connection.pragma_update(None, "wal_autocheckpoint", 256)?;
        connection.pragma_update(None, "journal_size_limit", 8 * 1024 * 1024)?;
        let physical_storage = Arc::new(PhysicalStorage::new(path, limits));
        physical_storage.configure(&connection)?;
        let image_artifact_root = (path != Path::new(":memory:")).then(|| {
            Arc::new(
                path.parent()
                    .expect("validated database path")
                    .join("image-artifacts"),
            )
        });
        let database = Self {
            connection: Arc::new(Mutex::new(connection)),
            search_reader: if path == Path::new(":memory:") {
                None
            } else {
                live_path_anchor.verify_path(path)?;
                let reader = Connection::open_with_flags(
                    path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                reader.busy_timeout(crate::chat_search::READER_BUSY_TIMEOUT)?;
                live_path_anchor.verify_path(path)?;
                Some(Arc::new(Mutex::new(reader)))
            },
            physical_storage,
            _live_path_anchor: Arc::new(live_path_anchor),
            image_artifact_root,
        };
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

    pub(crate) fn image_artifact_root(&self) -> AppResult<&Path> {
        self.image_artifact_root
            .as_deref()
            .map(|path| path.as_path())
            .ok_or_else(|| {
                crate::error::AppError::Unsupported("image artifacts need a disk database".into())
            })
    }

    pub(crate) fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn chat_search_reader(
        &self,
    ) -> AppResult<Option<std::sync::MutexGuard<'_, Connection>>> {
        self.search_reader
            .as_ref()
            .map(|reader| {
                reader.try_lock().map_err(|_| {
                    crate::error::AppError::ResourceLimit(
                        "Chat search is already reading a snapshot".into(),
                    )
                })
            })
            .transpose()
    }

    #[cfg(test)]
    pub(crate) fn connection_ownership_probe(&self) -> std::sync::Weak<Mutex<Connection>> {
        Arc::downgrade(&self.connection)
    }

    pub(crate) fn write_connection(&self) -> AppResult<WriteConnection<'_>> {
        let connection = self.connection();
        self.physical_storage.admit_write(&connection)?;
        Ok(WriteConnection {
            connection,
            physical_storage: &self.physical_storage,
        })
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture")))]
    pub(crate) fn converge_wal_for_observation(&self) -> AppResult<WalConvergenceReceipt> {
        let connection = self.connection();
        self.physical_storage.converge_for_observation(&connection)
    }
}

pub(crate) struct WriteConnection<'a> {
    connection: std::sync::MutexGuard<'a, Connection>,
    physical_storage: &'a PhysicalStorage,
}

impl std::ops::Deref for WriteConnection<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl std::ops::DerefMut for WriteConnection<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

impl Drop for WriteConnection<'_> {
    fn drop(&mut self) {
        self.physical_storage.observe_after_write(&self.connection);
    }
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
        assert_eq!(migration_count, 22);
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

    #[test]
    fn wal_observation_reports_a_below_target_disk_database_without_checkpointing() {
        let directory =
            std::env::temp_dir().join(format!("sage-wal-observation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = Database::open_with_limits(
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
        let database = Database::open_with_limits(
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
        let database = Database::open_with_limits(
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
            Err(crate::error::AppError::ResourceLimit(_))
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
        let database = Database::open_with_limits(
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
        assert!(database.physical_storage.recovery_required());
        assert!(matches!(
            database.write_connection(),
            Err(crate::error::AppError::ResourceLimit(_))
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
        assert!(!database.physical_storage.recovery_required());
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
        let database = Database::open(&path).unwrap();
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
        let database = Database::open(&root.join("sage.db")).unwrap();
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
