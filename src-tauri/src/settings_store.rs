use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
mod access;
mod notifications;
pub use notifications::NotificationPreference;
mod agent_rail_view;
mod browser_developer;
mod browser_download_folder;
mod browser_preferences;
mod browser_site_permissions;
mod chat_read_marks;
mod custom_acp_agents;
mod editor_settings;
mod new_chat_defaults;
mod side_chat;
mod terminal_dock;
pub use access::SessionAccessModes;
pub use agent_rail_view::AgentRailView;
pub use browser_download_folder::DownloadFolderRecord;
pub use browser_preferences::BrowserPreferences;
pub use browser_site_permissions::{MAX_SITE_ORIGIN_BYTES, MAX_SITE_PERMISSIONS};
#[cfg(test)]
pub(crate) use chat_read_marks::MAX_CHAT_READ_MARKS;
pub use chat_read_marks::{ChatReadMark, ChatReadMarkWrite, ChatReadMarks};
pub use custom_acp_agents::{
    CustomAcpAgent, CustomAcpAgentDraft, MAX_CUSTOM_ACP_AGENTS, custom_acp_number,
    validate_custom_acp_agent,
};
pub use editor_settings::{EditorSettings, EditorValue, MAX_GLOBS, default_files_exclude};
pub use new_chat_defaults::NewChatDefaults;

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
            // The fixed columns above are the legacy record; values now live
            // in the catalogue's rows, copied from it once.
            editor_settings::migrate(&migration)?;
            browser_preferences::migrate(&migration)?;
            browser_developer::migrate(&migration)?;
            agent_rail_view::migrate(&migration)?;
            chat_read_marks::migrate(&migration)?;
            browser_download_folder::migrate(&migration)?;
            browser_site_permissions::migrate(&migration)?;
            terminal_dock::migrate(&migration)?;
            new_chat_defaults::migrate(&migration)?;
            custom_acp_agents::migrate(&migration)?;
            migration.commit()?;
        }
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        Self::open(Path::new(":memory:"))
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
}
