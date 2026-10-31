//! The built-in browser's saved download folder, and the empty placeholder
//! files silent downloads have reserved. Rust alone writes both: the folder
//! comes from a native picker or the Save dialog, never from a renderer. The
//! stored path is only a hint; `browser::downloads::folder` re-admits it
//! (canonical, existing, writable) before every use.
//!
//! Every change to the folder (set or reset) advances `revision`, so the
//! first download's Save dialog can save its folder only if nothing changed
//! since it read the store, even a change and a change back.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use super::SettingsStore;
use crate::error::{AppError, AppResult};

const MAX_PATH_BYTES: usize = 4096;
/// More reservations than this are not recorded; they are still released
/// normally, only not recovered after a crash.
const MAX_RESERVATIONS: i64 = 64;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    // Earlier development builds created these tables without a revision
    // and without file identities; bring them to the current shape.
    if has_table(connection, "browser_download_folder")?
        && !has_column(connection, "browser_download_folder", "revision")?
    {
        connection.execute_batch(
            "ALTER TABLE browser_download_folder RENAME TO browser_download_folder_v1;",
        )?;
    }
    if has_table(connection, "browser_download_reservations")?
        && !has_column(connection, "browser_download_reservations", "file_index")?
    {
        // Records without an identity cannot prove a file is Sage's, so
        // their files are kept and the records dropped.
        connection.execute_batch("DROP TABLE browser_download_reservations;")?;
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_download_folder (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           path TEXT CHECK (path IS NULL OR length(path) BETWEEN 1 AND 4096),
           revision INTEGER NOT NULL CHECK (revision >= 0)
         ) STRICT;
         CREATE TABLE IF NOT EXISTS browser_download_reservations (
           path TEXT PRIMARY KEY CHECK (length(path) BETWEEN 1 AND 4096),
           volume INTEGER NOT NULL,
           file_index INTEGER NOT NULL
         ) STRICT;",
    )?;
    if has_table(connection, "browser_download_folder_v1")? {
        connection.execute_batch(
            "INSERT OR IGNORE INTO browser_download_folder(singleton, path, revision)
               SELECT singleton, path, 1 FROM browser_download_folder_v1;
             DROP TABLE browser_download_folder_v1;",
        )?;
    }
    Ok(())
}

fn has_table(connection: &Connection, table: &str) -> AppResult<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> AppResult<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2",
            params![table, column],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// A placeholder a silent download reserved: its path and which file it is
/// (volume and file index, or device and inode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadReservationRecord {
    pub path: PathBuf,
    pub volume: u64,
    pub file_index: u64,
}

/// The saved folder and the revision it was read at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadFolderRecord {
    pub path: Option<PathBuf>,
    pub revision: i64,
}

impl SettingsStore {
    pub fn browser_download_folder(&self) -> AppResult<Option<PathBuf>> {
        Ok(self.browser_download_folder_record()?.path)
    }

    pub fn browser_download_folder_record(&self) -> AppResult<DownloadFolderRecord> {
        read(&self.download_lock())
    }

    pub fn set_browser_download_folder(&self, folder: &Path) -> AppResult<()> {
        let text = admitted_text(folder)?;
        write(&self.download_lock(), Some(text))
    }

    /// Saves `folder` only while the record is still at `revision`, read and
    /// written under one lock.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn set_browser_download_folder_if(&self, revision: i64, folder: &Path) -> AppResult<bool> {
        let text = admitted_text(folder)?;
        let connection = self.download_lock();
        if read(&connection)?.revision != revision {
            return Ok(false);
        }
        write(&connection, Some(text))?;
        Ok(true)
    }

    pub fn clear_browser_download_folder(&self) -> AppResult<()> {
        write(&self.download_lock(), None)
    }

    /// Records a placeholder so a crash before its download settles does not
    /// leave it behind for good. Returns whether it was recorded.
    pub fn record_download_reservation(
        &self,
        record: &DownloadReservationRecord,
    ) -> AppResult<bool> {
        let text = admitted_text(&record.path)?;
        let connection = self.download_lock();
        let count: i64 = connection.query_row(
            "SELECT count(*) FROM browser_download_reservations",
            [],
            |row| row.get(0),
        )?;
        if count >= MAX_RESERVATIONS {
            return Ok(false);
        }
        // SQLite integers are signed; the bits round-trip unchanged.
        connection.execute(
            "INSERT OR REPLACE INTO browser_download_reservations(path, volume, file_index)
             VALUES (?1, ?2, ?3)",
            params![text, record.volume as i64, record.file_index as i64],
        )?;
        Ok(true)
    }

    pub fn forget_download_reservation(&self, path: &Path) -> AppResult<()> {
        let Some(text) = path.to_str() else {
            return Ok(());
        };
        self.download_lock().execute(
            "DELETE FROM browser_download_reservations WHERE path = ?1",
            params![text],
        )?;
        Ok(())
    }

    /// Every recorded placeholder. Records stay until their clean-up is
    /// confirmed with [`Self::forget_download_reservation`].
    pub fn download_reservations(&self) -> AppResult<Vec<DownloadReservationRecord>> {
        let connection = self.download_lock();
        let records = connection
            .prepare("SELECT path, volume, file_index FROM browser_download_reservations")?
            .query_map([], |row| {
                Ok(DownloadReservationRecord {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    volume: row.get::<_, i64>(1)? as u64,
                    file_index: row.get::<_, i64>(2)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    fn download_lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn read(connection: &Connection) -> AppResult<DownloadFolderRecord> {
    Ok(connection
        .query_row(
            "SELECT path, revision FROM browser_download_folder WHERE singleton = 1",
            [],
            |row| {
                Ok(DownloadFolderRecord {
                    path: row.get::<_, Option<String>>(0)?.map(PathBuf::from),
                    revision: row.get(1)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

fn write(connection: &Connection, text: Option<&str>) -> AppResult<()> {
    connection.execute(
        "INSERT INTO browser_download_folder(singleton, path, revision) VALUES (1, ?1, 1)
         ON CONFLICT(singleton) DO UPDATE
         SET path = excluded.path, revision = browser_download_folder.revision + 1",
        params![text],
    )?;
    Ok(())
}

fn admitted_text(path: &Path) -> AppResult<&str> {
    path.to_str()
        .filter(|text| path.is_absolute() && (1..=MAX_PATH_BYTES).contains(&text.len()))
        .ok_or_else(|| AppError::InvalidRequest("download path must be absolute".into()))
}

#[cfg(test)]
#[path = "browser_download_folder_tests.rs"]
mod tests;
