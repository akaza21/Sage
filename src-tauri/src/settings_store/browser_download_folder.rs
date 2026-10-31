//! The built-in browser's saved download folder. Rust alone writes it: the
//! path comes from a native picker or the Save dialog, never from a renderer.
//! The stored text is only a hint; `browser::downloads::folder` re-admits it
//! (canonical, existing, writable) before every use.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use super::SettingsStore;
use crate::error::{AppError, AppResult};

const MAX_PATH_BYTES: usize = 4096;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_download_folder (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           path TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096)
         ) STRICT;",
    )?;
    Ok(())
}

impl SettingsStore {
    pub fn browser_download_folder(&self) -> AppResult<Option<PathBuf>> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(connection
            .query_row(
                "SELECT path FROM browser_download_folder WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(PathBuf::from))
    }

    pub fn set_browser_download_folder(&self, folder: &Path) -> AppResult<()> {
        let text = folder
            .to_str()
            .filter(|text| folder.is_absolute() && (1..=MAX_PATH_BYTES).contains(&text.len()))
            .ok_or_else(|| {
                AppError::InvalidRequest("download folder must be an absolute path".into())
            })?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO browser_download_folder(singleton, path) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET path = excluded.path",
            params![text],
        )?;
        Ok(())
    }

    pub fn clear_browser_download_folder(&self) -> AppResult<()> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute("DELETE FROM browser_download_folder", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_download_folder_is_saved_until_one_is_set() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(store.browser_download_folder().unwrap(), None);
        store.clear_browser_download_folder().unwrap();
        assert_eq!(store.browser_download_folder().unwrap(), None);
    }

    #[test]
    fn the_download_folder_persists_until_reset() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        SettingsStore::open(&path)
            .unwrap()
            .set_browser_download_folder(&directory)
            .unwrap();
        let reopened = SettingsStore::open(&path).unwrap();
        assert_eq!(
            reopened.browser_download_folder().unwrap(),
            Some(directory.clone())
        );
        reopened.clear_browser_download_folder().unwrap();
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .browser_download_folder()
                .unwrap(),
            None
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_relative_or_oversized_download_folder_is_refused() {
        let store = SettingsStore::in_memory().unwrap();
        assert!(
            store
                .set_browser_download_folder(Path::new("relative/downloads"))
                .is_err()
        );
        let long = std::env::temp_dir().join("x".repeat(MAX_PATH_BYTES + 1));
        assert!(store.set_browser_download_folder(&long).is_err());
        let connection = store.0.lock().unwrap();
        assert!(
            connection
                .execute(
                    "INSERT INTO browser_download_folder(singleton, path) VALUES (1, '')",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO browser_download_folder(singleton, path) VALUES (2, '/x')",
                    [],
                )
                .is_err()
        );
    }
}
