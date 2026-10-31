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
        let text = admitted_text(folder)?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write(&connection, text)
    }

    /// Saves `folder` only while the stored value is still `expected`, read
    /// and written under one lock, so a folder chosen in Settings while a
    /// first Save dialog was open is never replaced by that dialog's folder.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn set_browser_download_folder_if(
        &self,
        expected: Option<&Path>,
        folder: &Path,
    ) -> AppResult<bool> {
        let text = admitted_text(folder)?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = connection
            .query_row(
                "SELECT path FROM browser_download_folder WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if current.as_deref().map(Path::new) != expected {
            return Ok(false);
        }
        write(&connection, text)?;
        Ok(true)
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

fn admitted_text(folder: &Path) -> AppResult<&str> {
    folder
        .to_str()
        .filter(|text| folder.is_absolute() && (1..=MAX_PATH_BYTES).contains(&text.len()))
        .ok_or_else(|| AppError::InvalidRequest("download folder must be an absolute path".into()))
}

fn write(connection: &Connection, text: &str) -> AppResult<()> {
    connection.execute(
        "INSERT INTO browser_download_folder(singleton, path) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET path = excluded.path",
        params![text],
    )?;
    Ok(())
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

    #[test]
    fn a_conditional_save_keeps_a_folder_saved_in_the_meantime() {
        let store = SettingsStore::in_memory().unwrap();
        let first = std::env::temp_dir().join("first");
        let settings = std::env::temp_dir().join("settings");
        store.set_browser_download_folder(&settings).unwrap();
        assert!(!store.set_browser_download_folder_if(None, &first).unwrap());
        assert_eq!(
            store.browser_download_folder().unwrap(),
            Some(settings.clone())
        );
        assert!(
            store
                .set_browser_download_folder_if(Some(&settings), &first)
                .unwrap()
        );
        assert_eq!(store.browser_download_folder().unwrap(), Some(first));
    }
}
