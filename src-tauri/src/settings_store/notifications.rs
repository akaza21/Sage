//! Native preference in the existing versioned settings rows; no new settings
//! schema. The editor catalogue ignores this key and cannot modify it.
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::SettingsStore;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationPreference {
    #[default]
    Off,
    BackgroundTurns,
}

impl SettingsStore {
    pub fn notification_preference(&self) -> AppResult<NotificationPreference> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let value: Option<(String, i64)> = connection
            .query_row(
                "SELECT value, version FROM editor_settings WHERE key = 'notifications.turnEvents'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match value {
            None => Ok(NotificationPreference::Off),
            Some((value, 1)) => serde_json::from_str(&value)
                .map_err(|_| AppError::InvalidRequest("invalid notification preference".into())),
            Some(_) => Err(AppError::InvalidRequest(
                "unsupported notification preference version".into(),
            )),
        }
    }

    pub fn set_notification_preference(
        &self,
        value: NotificationPreference,
    ) -> AppResult<NotificationPreference> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO editor_settings(key,value,version) VALUES ('notifications.turnEvents',?1,1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value,version=excluded.version",
            params![match value { NotificationPreference::Off => "\"off\"", NotificationPreference::BackgroundTurns => "\"backgroundTurns\"" }],
        )?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_defaults_off_and_survives_restart_without_exposing_an_editor_setting() {
        let directory = std::env::temp_dir().join(format!("sage-notices-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(
            store.notification_preference().unwrap(),
            NotificationPreference::Off
        );
        store
            .set_notification_preference(NotificationPreference::BackgroundTurns)
            .unwrap();
        assert!(
            !store
                .editor_settings()
                .unwrap()
                .contains_key("notifications.turnEvents")
        );
        drop(store);
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .notification_preference()
                .unwrap(),
            NotificationPreference::BackgroundTurns
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_or_future_values_fail_closed() {
        let store = SettingsStore::in_memory().unwrap();
        assert!(serde_json::from_str::<NotificationPreference>("\"always\"").is_err());
        store
            .0
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO editor_settings VALUES ('notifications.turnEvents','true',1)",
                [],
            )
            .unwrap();
        assert!(store.notification_preference().is_err());
        store
            .0
            .lock()
            .unwrap()
            .execute(
                "UPDATE editor_settings SET version=2 WHERE key='notifications.turnEvents'",
                [],
            )
            .unwrap();
        assert!(store.notification_preference().is_err());
    }
}
