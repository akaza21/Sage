//! Built-in browser preferences (BF-7d). One bounded record; every field is
//! closed and validated here, so the WebView never stores browser settings.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;
use serde::{Deserialize, Serialize};

use super::SettingsStore;

/// The Browser settings page's stored choices. Unknown fields are refused, so
/// a renderer cannot persist anything beyond this record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserPreferences {
    /// Show the path, query and fragment in the address bar, not only the host.
    pub show_full_url: bool,
}

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS browser_preferences (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           show_full_url INTEGER NOT NULL CHECK (show_full_url IN (0, 1))
         ) STRICT;",
        )
        .db()?;
    Ok(())
}

impl SettingsStore {
    pub fn browser_preferences(&self) -> AppResult<BrowserPreferences> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let show_full_url = connection
            .query_row(
                "SELECT show_full_url FROM browser_preferences WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .db()?;
        Ok(show_full_url
            .map(|show_full_url| BrowserPreferences { show_full_url })
            .unwrap_or_default())
    }

    pub fn set_browser_preferences(
        &self,
        value: BrowserPreferences,
    ) -> AppResult<BrowserPreferences> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection
            .execute(
                "INSERT INTO browser_preferences(singleton, show_full_url) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET show_full_url = excluded.show_full_url",
                params![value.show_full_url],
            )
            .db()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_preferences_default_to_the_host_only_address() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(
            store.browser_preferences().unwrap(),
            BrowserPreferences {
                show_full_url: false
            }
        );
    }

    #[test]
    fn browser_preferences_persist_across_reopening_the_store() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-browser-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let value = BrowserPreferences {
            show_full_url: true,
        };
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .set_browser_preferences(value)
                .unwrap(),
            value
        );
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .browser_preferences()
                .unwrap(),
            value
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_settings_store_from_before_browser_preferences_gains_the_table_and_keeps_its_rows() {
        let directory = std::env::temp_dir().join(format!(
            "sage-settings-browser-migrate-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        {
            // The ui_appearance record as an earlier Sage wrote it, with no
            // browser_preferences table.
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE ui_appearance (
                       singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                       font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
                       density TEXT NOT NULL CHECK (density IN ('comfortable', 'compact'))
                     ) STRICT;
                     INSERT INTO ui_appearance VALUES (1, 16, 'compact');",
                )
                .unwrap();
        }
        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(
            store.browser_preferences().unwrap(),
            BrowserPreferences::default()
        );
        assert_eq!(
            store.ui_appearance().unwrap(),
            super::super::UiAppearance {
                font_size: 16,
                density: super::super::UiDensity::Compact,
            }
        );
        let value = BrowserPreferences {
            show_full_url: true,
        };
        store.set_browser_preferences(value).unwrap();
        drop(store);
        // Opening again leaves the migrated table and its row alone.
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .browser_preferences()
                .unwrap(),
            value
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn browser_preferences_refuse_unknown_fields_and_wrong_types() {
        for input in [
            serde_json::json!({ "showFullUrl": true, "cookies": "a=b" }),
            serde_json::json!({ "showFullUrl": "yes" }),
            serde_json::json!({ "showFullUrl": 1 }),
            serde_json::json!({}),
            serde_json::json!(null),
        ] {
            assert!(
                serde_json::from_value::<BrowserPreferences>(input.clone()).is_err(),
                "{input}"
            );
        }
        assert_eq!(
            serde_json::from_value::<BrowserPreferences>(
                serde_json::json!({ "showFullUrl": true })
            )
            .unwrap(),
            BrowserPreferences {
                show_full_url: true
            }
        );
    }

    #[test]
    fn the_browser_preferences_table_admits_only_booleans() {
        let store = SettingsStore::in_memory().unwrap();
        let connection = store.0.lock().unwrap();
        assert!(
            connection
                .execute(
                    "INSERT INTO browser_preferences(singleton, show_full_url) VALUES (1, 2)",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO browser_preferences(singleton, show_full_url) VALUES (2, 1)",
                    [],
                )
                .is_err()
        );
    }
}
