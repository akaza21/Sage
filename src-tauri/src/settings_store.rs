use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
mod access;
pub use access::SessionAccessModes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAppearance {
    pub font_size: u8,
    pub tab_size: u8,
    pub word_wrap: bool,
    #[serde(default = "default_line_numbers")]
    pub line_numbers: bool,
    #[serde(default)]
    pub show_whitespace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionAccessMode {
    ReadOnly,
    Ask,
    WorkspaceWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiDensity {
    Comfortable,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiAppearance {
    pub font_size: u8,
    pub density: UiDensity,
}

impl Default for UiAppearance {
    fn default() -> Self {
        Self {
            font_size: 14,
            density: UiDensity::Comfortable,
        }
    }
}

const fn default_line_numbers() -> bool {
    true
}

impl Default for EditorAppearance {
    fn default() -> Self {
        Self {
            font_size: 13,
            tab_size: 2,
            word_wrap: false,
            line_numbers: true,
            show_whitespace: false,
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

fn admitted_ui(value: UiAppearance) -> AppResult<UiAppearance> {
    if !matches!(value.font_size, 12 | 13 | 14 | 16) {
        return Err(AppError::InvalidRequest(
            "ui appearance is outside the supported values".into(),
        ));
    }
    Ok(value)
}

#[derive(Debug, Clone)]
pub struct SettingsStore(Arc<Mutex<Connection>>);

impl SettingsStore {
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        {
            let migration = connection.transaction()?;
            migration.execute_batch(
                "CREATE TABLE IF NOT EXISTS editor_appearance (
                   singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                   font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
                   tab_size INTEGER NOT NULL CHECK (tab_size IN (2, 4, 8)),
                   word_wrap INTEGER NOT NULL CHECK (word_wrap IN (0, 1)),
                   line_numbers INTEGER NOT NULL DEFAULT 1 CHECK (line_numbers IN (0, 1)),
                   show_whitespace INTEGER NOT NULL DEFAULT 0 CHECK (show_whitespace IN (0, 1))
                 ) STRICT;",
            )?;
            migration.execute_batch(
                "CREATE TABLE IF NOT EXISTS session_access_mode (
                   singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                   mode TEXT NOT NULL CHECK (mode IN ('readOnly', 'ask', 'workspaceWrite'))
                 ) STRICT;",
            )?;
            migration.execute_batch("CREATE TABLE IF NOT EXISTS binding_access_modes (binding_id TEXT PRIMARY KEY CHECK(length(binding_id) BETWEEN 1 AND 128), mode TEXT NOT NULL CHECK(mode IN ('readOnly','ask','workspaceWrite'))) STRICT;")?;
            migration.execute_batch(
                "CREATE TABLE IF NOT EXISTS ui_appearance (
                   singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                   font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
                   density TEXT NOT NULL CHECK (density IN ('comfortable', 'compact'))
                 ) STRICT;",
            )?;
            if !table_has_column(&migration, "line_numbers")? {
                migration.execute_batch(
                    "ALTER TABLE editor_appearance
                     ADD COLUMN line_numbers INTEGER NOT NULL DEFAULT 1
                     CHECK (line_numbers IN (0, 1));",
                )?;
            }
            if !table_has_column(&migration, "show_whitespace")? {
                migration.execute_batch(
                    "ALTER TABLE editor_appearance
                     ADD COLUMN show_whitespace INTEGER NOT NULL DEFAULT 0
                     CHECK (show_whitespace IN (0, 1));",
                )?;
            }
            migration.commit()?;
        }
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
                "SELECT font_size, tab_size, word_wrap, line_numbers, show_whitespace
                 FROM editor_appearance WHERE singleton = 1",
                [],
                |row| {
                    Ok(EditorAppearance {
                        font_size: row.get(0)?,
                        tab_size: row.get(1)?,
                        word_wrap: row.get(2)?,
                        line_numbers: row.get(3)?,
                        show_whitespace: row.get(4)?,
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
            "INSERT INTO editor_appearance(
               singleton, font_size, tab_size, word_wrap, line_numbers, show_whitespace
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(singleton) DO UPDATE SET
               font_size=excluded.font_size,
               tab_size=excluded.tab_size,
               word_wrap=excluded.word_wrap,
               line_numbers=excluded.line_numbers,
               show_whitespace=excluded.show_whitespace",
            params![
                value.font_size,
                value.tab_size,
                value.word_wrap,
                value.line_numbers,
                value.show_whitespace
            ],
        )?;
        Ok(value)
    }

    pub fn ui_appearance(&self) -> AppResult<UiAppearance> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection
            .query_row(
                "SELECT font_size, density FROM ui_appearance WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, u8>(0)?, row.get::<_, String>(1)?)),
            )
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok((14, "comfortable".to_owned())),
                error => Err(error),
            })
            .and_then(|(font_size, density)| {
                let density =
                    serde_json::from_value(serde_json::Value::String(density)).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(std::fmt::Error),
                        )
                    })?;
                Ok(UiAppearance { font_size, density })
            })
            .map_err(Into::into)
    }

    pub fn set_ui_appearance(&self, value: UiAppearance) -> AppResult<UiAppearance> {
        let value = admitted_ui(value)?;
        let density = match value.density {
            UiDensity::Comfortable => "comfortable",
            UiDensity::Compact => "compact",
        };
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO ui_appearance(singleton, font_size, density) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               font_size=excluded.font_size,
               density=excluded.density",
            params![value.font_size, density],
        )?;
        Ok(value)
    }
}

fn table_has_column(connection: &Connection, column: &str) -> AppResult<bool> {
    let mut statement = connection.prepare("PRAGMA table_info(editor_appearance)")?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_only_the_bounded_ui_appearance_values() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(store.ui_appearance().unwrap(), UiAppearance::default());
        let value = UiAppearance {
            font_size: 16,
            density: UiDensity::Compact,
        };
        assert_eq!(store.set_ui_appearance(value).unwrap(), value);
        assert_eq!(store.ui_appearance().unwrap(), value);
        assert!(
            store
                .set_ui_appearance(UiAppearance {
                    font_size: 11,
                    ..value
                })
                .is_err()
        );
    }

    #[test]
    fn restores_the_ui_appearance_after_reopening_its_sqlite_store() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let value = UiAppearance {
            font_size: 12,
            density: UiDensity::Compact,
        };
        SettingsStore::open(&path)
            .unwrap()
            .set_ui_appearance(value)
            .unwrap();
        assert_eq!(
            SettingsStore::open(&path).unwrap().ui_appearance().unwrap(),
            value
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

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
            line_numbers: false,
            show_whitespace: true,
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
            line_numbers: false,
            show_whitespace: true,
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

    #[test]
    fn migrates_the_legacy_three_column_record_without_losing_values() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE editor_appearance (
                       singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                       font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
                       tab_size INTEGER NOT NULL CHECK (tab_size IN (2, 4, 8)),
                       word_wrap INTEGER NOT NULL CHECK (word_wrap IN (0, 1))
                     ) STRICT;
                     INSERT INTO editor_appearance(singleton, font_size, tab_size, word_wrap)
                     VALUES (1, 16, 8, 1);",
                )
                .unwrap();
        }

        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(
            store.editor_appearance().unwrap(),
            EditorAppearance {
                font_size: 16,
                tab_size: 8,
                word_wrap: true,
                line_numbers: true,
                show_whitespace: false,
            }
        );
        drop(store);

        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .editor_appearance()
                .unwrap(),
            EditorAppearance {
                font_size: 16,
                tab_size: 8,
                word_wrap: true,
                line_numbers: true,
                show_whitespace: false,
            }
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deserializes_a_legacy_frontend_record_with_display_defaults() {
        let appearance: EditorAppearance = serde_json::from_value(serde_json::json!({
            "fontSize": 14,
            "tabSize": 4,
            "wordWrap": true
        }))
        .unwrap();

        assert_eq!(
            appearance,
            EditorAppearance {
                font_size: 14,
                tab_size: 4,
                word_wrap: true,
                line_numbers: true,
                show_whitespace: false,
            }
        );
    }
}
