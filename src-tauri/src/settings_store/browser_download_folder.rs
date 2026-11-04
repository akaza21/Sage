//! The built-in browser's saved download folder. Rust alone writes it: the
//! folder comes from a native picker or the Save dialog, never from a
//! renderer. The stored path is only a hint; `browser::downloads::folder` re-admits it
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
    // Earlier development builds also kept placeholder records for a crash
    // sweep; Sage no longer deletes anything after a crash.
    connection.execute_batch("DROP TABLE IF EXISTS browser_download_reservations;")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_download_folder (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           path TEXT CHECK (path IS NULL OR length(path) BETWEEN 1 AND 4096),
           revision INTEGER NOT NULL CHECK (revision >= 0)
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

/// The saved folder and the revision it was read at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadFolderRecord {
    pub path: Option<PathBuf>,
    pub revision: i64,
}

impl SettingsStore {
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
