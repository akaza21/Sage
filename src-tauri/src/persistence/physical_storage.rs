use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use rusqlite::Connection;
use sage_core::error::{AppError, AppResult};

use sage_core::limits::LIMITS;

#[cfg(any(test, all(windows, feature = "acp-fixture")))]
use super::WalConvergenceReceipt;

#[derive(Debug, Clone, Copy)]
pub(super) struct StorageLimits {
    main_bytes: u64,
    checkpoint_bytes: u64,
    wal_hard_bytes: u64,
}

impl StorageLimits {
    pub(super) fn production() -> Self {
        Self {
            main_bytes: LIMITS.migration.live_sqlite_bytes,
            checkpoint_bytes: LIMITS.migration.wal_checkpoint_target_bytes,
            wal_hard_bytes: LIMITS.migration.wal_hard_stop_bytes,
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(main_bytes: u64, checkpoint_bytes: u64, wal_hard_bytes: u64) -> Self {
        Self {
            main_bytes,
            checkpoint_bytes,
            wal_hard_bytes,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    recovery_required: bool,
}

#[derive(Debug)]
pub(super) struct PhysicalStorage {
    path: Option<PathBuf>,
    limits: StorageLimits,
    state: Mutex<State>,
}

impl PhysicalStorage {
    pub(super) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(super) fn new(path: &Path, limits: StorageLimits) -> Self {
        Self {
            path: (path != Path::new(":memory:")).then(|| path.to_owned()),
            limits,
            state: Mutex::new(State::default()),
        }
    }

    pub(super) fn configure(&self, connection: &Connection) -> AppResult<()> {
        let page_size: i64 = connection.pragma_query_value(None, "page_size", |row| row.get(0))?;
        let page_size = u64::try_from(page_size.max(1)).unwrap_or(1);
        let max_pages =
            i64::try_from((self.limits.main_bytes / page_size).max(1)).map_err(|_| {
                AppError::ResourceLimit("SQLite page limit is not representable".into())
            })?;
        connection.pragma_update(None, "max_page_count", max_pages)?;
        Ok(())
    }

    pub(super) fn admit_write(&self, connection: &Connection) -> AppResult<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut state = lock(&self.state);
        let (main, mut wal) = physical_bytes(path)?;
        if main > self.limits.main_bytes {
            state.recovery_required = true;
            return storage_limit("SQLite database exceeds its physical limit");
        }
        if wal >= self.limits.checkpoint_bytes || state.recovery_required {
            checkpoint(connection, "PASSIVE")?;
            wal = sidecar_len(path, "-wal")?;
        }
        if wal > self.limits.wal_hard_bytes {
            checkpoint(connection, "TRUNCATE")?;
            wal = sidecar_len(path, "-wal")?;
        }
        if wal > self.limits.wal_hard_bytes {
            state.recovery_required = true;
            return storage_limit("SQLite WAL exceeded its hard stop; recovery is required");
        }
        if state.recovery_required && wal > self.limits.checkpoint_bytes {
            return storage_limit("SQLite WAL has not converged; recovery is required");
        }
        state.recovery_required = false;
        Ok(())
    }

    pub(super) fn observe_after_write(&self, connection: &Connection) {
        let Some(path) = &self.path else { return };
        let Ok((main, wal)) = physical_bytes(path) else {
            lock(&self.state).recovery_required = true;
            return;
        };
        if main > self.limits.main_bytes || wal > self.limits.wal_hard_bytes {
            lock(&self.state).recovery_required = true;
            return;
        }
        if wal >= self.limits.checkpoint_bytes {
            let _ = checkpoint(connection, "PASSIVE");
            if sidecar_len(path, "-wal").is_ok_and(|bytes| bytes > self.limits.wal_hard_bytes) {
                lock(&self.state).recovery_required = true;
            }
        }
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture")))]
    pub(super) fn converge_for_observation(
        &self,
        connection: &Connection,
    ) -> AppResult<WalConvergenceReceipt> {
        let path = self.path.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("WAL observation requires a disk database".into())
        })?;
        let initial = sidecar_len(path, "-wal")?;
        let checkpointed = initial >= self.limits.checkpoint_bytes;
        if checkpointed && checkpoint(connection, "TRUNCATE")? {
            return storage_limit("SQLite WAL checkpoint remained busy during observation");
        }
        let wal_bytes = sidecar_len(path, "-wal")?;
        if wal_bytes >= self.limits.checkpoint_bytes {
            return storage_limit("SQLite WAL did not converge below its observation target");
        }
        Ok(WalConvergenceReceipt {
            wal_bytes,
            checkpoint_target_bytes: self.limits.checkpoint_bytes,
            checkpointed,
        })
    }

    #[cfg(test)]
    pub(super) fn recovery_required(&self) -> bool {
        lock(&self.state).recovery_required
    }
}

fn checkpoint(connection: &Connection, mode: &str) -> AppResult<bool> {
    let sql = format!("PRAGMA wal_checkpoint({mode})");
    let result: (i64, i64, i64) =
        connection.query_row(&sql, [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    Ok(result.0 != 0)
}

fn physical_bytes(path: &Path) -> AppResult<(u64, u64)> {
    Ok((fs::metadata(path)?.len(), sidecar_len(path, "-wal")?))
}

fn sidecar_len(path: &Path, suffix: &str) -> AppResult<u64> {
    let mut name = path
        .file_name()
        .ok_or_else(|| AppError::InvalidRequest("Sage database path has no filename".into()))?
        .to_os_string();
    name.push(suffix);
    match fs::metadata(path.with_file_name(name)) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn storage_limit<T>(message: &str) -> AppResult<T> {
    Err(AppError::ResourceLimit(message.into()))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
