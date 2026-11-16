use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::Connection;
use sage_core::error::AppResult;

#[cfg(any(feature = "test-support", all(windows, feature = "fixture")))]
use crate::physical_storage::WalConvergenceReceipt;
use crate::{
    error::SqliteResultExt,
    physical_storage::{PhysicalStorage, StorageLimits},
};

/// Proves the database file at a path is still the one the migration opened
/// and anchored. The migration lives in the app, so it hands its anchor in.
pub trait PathAnchor: std::fmt::Debug + Send + Sync {
    fn verify_path(&self, path: &Path) -> AppResult<()>;
}

/// How `Database::open` sets up the connections it hands out.
#[derive(Debug, Clone, Copy)]
pub struct DatabaseOptions {
    /// How long the chat search reader waits for a writer's lock.
    pub reader_busy_timeout: Duration,
    pub storage_limits: StorageLimits,
}

#[derive(Debug, Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
    search_reader: Option<Arc<Mutex<Connection>>>,
    physical_storage: Arc<PhysicalStorage>,
    live_path_anchor: Arc<dyn PathAnchor>,
    image_artifact_root: Option<Arc<PathBuf>>,
}

impl Database {
    /// Takes the connection the migration brought up to date (after the
    /// app's crash recovery has run on it), applies the connection settings
    /// and storage limits, and opens the chat search reader.
    pub fn open(
        path: &Path,
        connection: Connection,
        live_path_anchor: impl PathAnchor + 'static,
        options: DatabaseOptions,
    ) -> AppResult<Self> {
        connection.pragma_update(None, "journal_mode", "WAL").db()?;
        connection.pragma_update(None, "foreign_keys", "ON").db()?;
        connection.pragma_update(None, "busy_timeout", 5_000).db()?;
        connection
            .pragma_update(None, "wal_autocheckpoint", 256)
            .db()?;
        connection
            .pragma_update(None, "journal_size_limit", 8 * 1024 * 1024)
            .db()?;
        let physical_storage = Arc::new(PhysicalStorage::new(path, options.storage_limits));
        physical_storage.configure(&connection)?;
        let image_artifact_root = (path != Path::new(":memory:")).then(|| {
            Arc::new(
                path.parent()
                    .expect("validated database path")
                    .join("image-artifacts"),
            )
        });
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            search_reader: if path == Path::new(":memory:") {
                None
            } else {
                live_path_anchor.verify_path(path)?;
                let reader = Connection::open_with_flags(
                    path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )
                .db()?;
                reader.busy_timeout(options.reader_busy_timeout).db()?;
                live_path_anchor.verify_path(path)?;
                Some(Arc::new(Mutex::new(reader)))
            },
            physical_storage,
            live_path_anchor: Arc::new(live_path_anchor),
            image_artifact_root,
        })
    }

    pub fn image_artifact_root(&self) -> AppResult<&Path> {
        self.image_artifact_root
            .as_deref()
            .map(|path| path.as_path())
            .ok_or_else(|| {
                sage_core::error::AppError::Unsupported(
                    "image artifacts need a disk database".into(),
                )
            })
    }

    /// A read-only connection holding an open read transaction, so it keeps
    /// seeing the store as it was at this call while writers go on. Cheap: it
    /// only fixes the WAL snapshot. `None` for an in-memory store.
    pub fn pinned_reader(&self) -> AppResult<Option<Connection>> {
        let Some(path) = self.physical_storage.path() else {
            return Ok(None);
        };
        self.live_path_anchor.verify_path(path)?;
        let reader = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .db()?;
        self.live_path_anchor.verify_path(path)?;
        // A deferred transaction takes its snapshot at the first read.
        reader
            .execute_batch("BEGIN; SELECT count(*) FROM sqlite_schema;")
            .db()?;
        Ok(Some(reader))
    }

    pub fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn chat_search_reader(&self) -> AppResult<Option<std::sync::MutexGuard<'_, Connection>>> {
        self.search_reader
            .as_ref()
            .map(|reader| {
                reader.try_lock().map_err(|_| {
                    sage_core::error::AppError::ResourceLimit(
                        "Chat search is already reading a snapshot".into(),
                    )
                })
            })
            .transpose()
    }

    #[cfg(feature = "test-support")]
    pub fn connection_ownership_probe(&self) -> std::sync::Weak<Mutex<Connection>> {
        Arc::downgrade(&self.connection)
    }

    /// Whether the physical storage gate holds writes until the WAL
    /// converges.
    #[cfg(feature = "test-support")]
    pub fn storage_recovery_required(&self) -> bool {
        self.physical_storage.recovery_required()
    }

    pub fn write_connection(&self) -> AppResult<WriteConnection<'_>> {
        let connection = self.connection();
        self.physical_storage.admit_write(&connection)?;
        Ok(WriteConnection {
            connection,
            physical_storage: &self.physical_storage,
        })
    }

    #[cfg(any(feature = "test-support", all(windows, feature = "fixture")))]
    pub fn converge_wal_for_observation(&self) -> AppResult<WalConvergenceReceipt> {
        let connection = self.connection();
        self.physical_storage.converge_for_observation(&connection)
    }
}

pub struct WriteConnection<'a> {
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
