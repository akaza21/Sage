use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAppearance {
    pub font_size: u8,
    pub tab_size: u8,
    pub word_wrap: bool,
}

impl Default for EditorAppearance {
    fn default() -> Self {
        Self {
            font_size: 13,
            tab_size: 2,
            word_wrap: false,
        }
    }
}

fn admitted(value: EditorAppearance) -> AppResult<EditorAppearance> {
    if !matches!(value.font_size, 12 | 13 | 14 | 16) || !matches!(value.tab_size, 2 | 4 | 8) {
        return Err(AppError::InvalidRequest(
            "editor appearance is outside the supported values".into(),
        ));
    }
    Ok(value)
}

#[derive(Debug, Clone)]
pub struct SettingsStore(Arc<Mutex<Connection>>);

impl SettingsStore {
    pub fn open(path: &Path) -> AppResult<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS editor_appearance (
               singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
               font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
               tab_size INTEGER NOT NULL CHECK (tab_size IN (2, 4, 8)),
               word_wrap INTEGER NOT NULL CHECK (word_wrap IN (0, 1))
             ) STRICT;",
        )?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        Self::open(Path::new(":memory:"))
    }

    pub fn editor_appearance(&self) -> AppResult<EditorAppearance> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection
            .query_row(
                "SELECT font_size, tab_size, word_wrap FROM editor_appearance WHERE singleton = 1",
                [],
                |row| {
                    Ok(EditorAppearance {
                        font_size: row.get(0)?,
                        tab_size: row.get(1)?,
                        word_wrap: row.get(2)?,
                    })
                },
            )
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(EditorAppearance::default()),
                error => Err(error),
            })
            .map_err(Into::into)
    }

    pub fn set_editor_appearance(&self, value: EditorAppearance) -> AppResult<EditorAppearance> {
        let value = admitted(value)?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO editor_appearance(singleton, font_size, tab_size, word_wrap)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET font_size=excluded.font_size, tab_size=excluded.tab_size, word_wrap=excluded.word_wrap",
            params![value.font_size, value.tab_size, value.word_wrap],
        )?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_only_the_bounded_editor_appearance_values() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(
            store.editor_appearance().unwrap(),
            EditorAppearance::default()
        );
        let value = EditorAppearance {
            font_size: 16,
            tab_size: 8,
            word_wrap: true,
        };
        assert_eq!(store.set_editor_appearance(value).unwrap(), value);
        assert_eq!(store.editor_appearance().unwrap(), value);
        assert!(
            store
                .set_editor_appearance(EditorAppearance {
                    font_size: 11,
                    ..value
                })
                .is_err()
        );
    }

    #[test]
    fn restores_the_appearance_after_reopening_its_sqlite_store() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let value = EditorAppearance {
            font_size: 16,
            tab_size: 4,
            word_wrap: true,
        };
        SettingsStore::open(&path)
            .unwrap()
            .set_editor_appearance(value)
            .unwrap();
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .editor_appearance()
                .unwrap(),
            value
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
