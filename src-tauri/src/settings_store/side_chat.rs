use super::SettingsStore;
use crate::error::AppResult;
use rusqlite::OptionalExtension;

impl SettingsStore {
    pub fn confirm_close_side_chat(&self) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute_batch("CREATE TABLE IF NOT EXISTS side_chat_preferences (singleton INTEGER PRIMARY KEY CHECK(singleton=1), confirm_close INTEGER NOT NULL CHECK(confirm_close IN (0,1))) STRICT;")?;
        Ok(connection
            .query_row(
                "SELECT confirm_close FROM side_chat_preferences WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(true))
    }
    pub fn set_confirm_close_side_chat(&self, confirm: bool) -> AppResult<()> {
        self.confirm_close_side_chat()?;
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute("INSERT INTO side_chat_preferences VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET confirm_close=excluded.confirm_close", [confirm])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_defaults_on_and_is_persisted() {
        let directory =
            std::env::temp_dir().join(format!("sage-side-preference-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let settings = SettingsStore::open(&path).unwrap();
        assert!(settings.confirm_close_side_chat().unwrap());
        settings.set_confirm_close_side_chat(false).unwrap();
        drop(settings);
        assert!(
            !SettingsStore::open(&path)
                .unwrap()
                .confirm_close_side_chat()
                .unwrap()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
