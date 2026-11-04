use super::*;
use rusqlite::OptionalExtension;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS terminal_dock (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           visible INTEGER NOT NULL CHECK (visible IN (0, 1))
         ) STRICT;",
    )?;
    Ok(())
}

impl SettingsStore {
    pub fn terminal_dock_visible(&self) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(connection
            .query_row(
                "SELECT visible FROM terminal_dock WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn set_terminal_dock_visible(&self, visible: bool) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO terminal_dock(singleton, visible) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET visible=excluded.visible",
            [visible],
        )?;
        Ok(visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_dock_defaults_closed_and_round_trips_after_reopen() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-terminal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        {
            let store = SettingsStore::open(&path).unwrap();
            assert!(!store.terminal_dock_visible().unwrap());
            assert!(store.set_terminal_dock_visible(true).unwrap());
        }
        {
            let store = SettingsStore::open(&path).unwrap();
            assert!(store.terminal_dock_visible().unwrap());
            assert!(!store.set_terminal_dock_visible(false).unwrap());
        }
        assert!(
            !SettingsStore::open(&path)
                .unwrap()
                .terminal_dock_visible()
                .unwrap()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
