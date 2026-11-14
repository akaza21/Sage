use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};
use sage_core::error::{AppError, AppResult};

use super::{SessionAccessMode, SettingsStore};

pub type SessionAccessModes = BTreeMap<String, SessionAccessMode>;

/// The stored defaults admit every mode a new chat may start in. Full access
/// is granted per chat session (`full_access.rs`) and is never a default, so
/// the schema itself refuses it.
macro_rules! default_modes {
    () => {
        "'readOnly','ask','workspaceWrite','acceptEdits','plan'"
    };
}
const DEFAULT_MODES: &str = default_modes!();
const BINDING_ACCESS_MODES: &str = concat!(
    "CREATE TABLE binding_access_modes (binding_id TEXT PRIMARY KEY CHECK(length(binding_id) BETWEEN 1 AND 128), mode TEXT NOT NULL CHECK(mode IN (",
    default_modes!(),
    "))) STRICT;"
);

pub(super) fn migrate(connection: &rusqlite::Connection) -> AppResult<()> {
    let existing: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name='binding_access_modes'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match existing {
        None => connection.execute_batch(BINDING_ACCESS_MODES)?,
        Some(sql) if sql == BINDING_ACCESS_MODES.trim_end_matches(';') => {}
        // Another build admitted other modes. Rebuild with this build's
        // check, keeping every choice it knows; a choice it does not know
        // (from a newer build) falls back to the binding's default.
        Some(_) => connection.execute_batch(&format!(
            "ALTER TABLE binding_access_modes RENAME TO binding_access_modes_previous;
             {BINDING_ACCESS_MODES}
             INSERT INTO binding_access_modes(binding_id,mode)
               SELECT binding_id,mode FROM binding_access_modes_previous
               WHERE mode IN ({DEFAULT_MODES});
             DROP TABLE binding_access_modes_previous;"
        ))?,
    }
    Ok(())
}

impl SettingsStore {
    pub fn session_access_modes(&self) -> AppResult<SessionAccessModes> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if migration_pending(&connection)? {
            return Err(AppError::InvalidRequest(
                "Access preferences must be restored before sending.".into(),
            ));
        }
        read_modes(&connection)
    }

    pub fn migrate_session_access_modes(
        &self,
        restored_binding: Option<&str>,
    ) -> AppResult<SessionAccessModes> {
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        if let Some(binding) = restored_binding {
            transaction.execute(
                "INSERT OR IGNORE INTO binding_access_modes(binding_id,mode)
                SELECT ?1,mode FROM session_access_mode WHERE singleton=1",
                [binding],
            )?;
        }
        transaction.execute("DELETE FROM session_access_mode WHERE singleton=1", [])?;
        let modes = read_modes(&transaction)?;
        transaction.commit()?;
        Ok(modes)
    }

    pub fn set_session_access_modes(
        &self,
        modes: &SessionAccessModes,
    ) -> AppResult<SessionAccessModes> {
        validate(modes)?;
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM binding_access_modes", [])?;
        for (binding, mode) in modes {
            let mode = serde_json::to_value(mode).expect("access mode serializes");
            transaction.execute(
                "INSERT INTO binding_access_modes(binding_id,mode) VALUES (?1,?2)",
                params![binding, mode.as_str()],
            )?;
        }
        transaction.commit()?;
        Ok(modes.clone())
    }

    pub fn pending_access_migration(&self) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        migration_pending(&connection)
    }
}

fn migration_pending(connection: &rusqlite::Connection) -> AppResult<bool> {
    Ok(connection
        .query_row(
            "SELECT singleton FROM session_access_mode WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

fn read_modes(connection: &rusqlite::Connection) -> AppResult<SessionAccessModes> {
    let mut statement = connection
        .prepare("SELECT binding_id,mode FROM binding_access_modes ORDER BY binding_id LIMIT 9")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut modes = BTreeMap::new();
    for row in rows {
        let (binding, mode) = row?;
        // A mode this build does not know (a newer build wrote it) is left
        // out, so that binding falls back to its least permissive default.
        match serde_json::from_value::<SessionAccessMode>(serde_json::Value::String(mode)) {
            Ok(mode) if mode.is_default_eligible() => {
                modes.insert(binding, mode);
            }
            _ => tracing::warn!(
                binding_id = binding.as_str(),
                "Unknown stored access default; using the binding's fallback"
            ),
        }
    }
    validate(&modes)?;
    Ok(modes)
}

fn validate(modes: &SessionAccessModes) -> AppResult<()> {
    if modes.values().any(|mode| !mode.is_default_eligible()) {
        return Err(AppError::InvalidRequest(
            "Full access applies to one chat and can't be saved as a default.".into(),
        ));
    }
    if modes.len() > 8
        || modes
            .keys()
            .any(|binding| binding.is_empty() || binding.len() > 128)
    {
        return Err(AppError::InvalidRequest(
            "Access preferences exceed their binding limit.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_store::SessionAccessMode::{
        AcceptEdits, Ask, Auto, FullAccess, Plan, ReadOnly, WorkspaceWrite,
    };

    #[test]
    fn no_mode_that_needs_a_confirmation_is_a_stored_default() {
        let store = SettingsStore::in_memory().unwrap();
        for (mode, id) in [(FullAccess, "fullAccess"), (Auto, "auto")] {
            assert!(!mode.is_default_eligible());
            let modes = BTreeMap::from([("claude-agent-acp".into(), mode)]);
            assert!(store.set_session_access_modes(&modes).is_err());
            assert!(store.session_access_modes().unwrap().is_empty());
            // The schema refuses it too, so no other writer can store one.
            assert!(
                store
                    .0
                    .lock()
                    .unwrap()
                    .execute(
                        "INSERT INTO binding_access_modes VALUES ('claude-agent-acp',?1)",
                        [id],
                    )
                    .is_err()
            );
        }
        let modes = BTreeMap::from([
            ("claude-agent-acp".into(), AcceptEdits),
            ("other".into(), Plan),
        ]);
        assert_eq!(store.set_session_access_modes(&modes).unwrap(), modes);
    }

    #[test]
    fn an_earlier_narrow_table_is_widened_with_its_choices_kept() {
        let directory = std::env::temp_dir().join(format!("sage-access-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.sqlite");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE binding_access_modes (binding_id TEXT PRIMARY KEY CHECK(length(binding_id) BETWEEN 1 AND 128), mode TEXT NOT NULL CHECK(mode IN ('readOnly','ask','workspaceWrite'))) STRICT;
                 INSERT INTO binding_access_modes VALUES ('codex-app-server','workspaceWrite');",
            )
            .unwrap();
        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(
            store.session_access_modes().unwrap(),
            BTreeMap::from([("codex-app-server".into(), WorkspaceWrite)])
        );
        let modes = BTreeMap::from([
            ("claude-agent-acp".into(), AcceptEdits),
            ("codex-app-server".into(), WorkspaceWrite),
        ]);
        store.set_session_access_modes(&modes).unwrap();
        drop(store);
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .session_access_modes()
                .unwrap(),
            modes
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_stored_auto_default_is_dropped_on_open() {
        let directory = std::env::temp_dir().join(format!("sage-access-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.sqlite");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE binding_access_modes (binding_id TEXT PRIMARY KEY CHECK(length(binding_id) BETWEEN 1 AND 128), mode TEXT NOT NULL CHECK(mode IN ('readOnly','ask','workspaceWrite','acceptEdits','plan','auto'))) STRICT;
                 INSERT INTO binding_access_modes VALUES ('claude-agent-acp','auto');
                 INSERT INTO binding_access_modes VALUES ('codex-app-server','workspaceWrite');",
            )
            .unwrap();
        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(
            store.session_access_modes().unwrap(),
            BTreeMap::from([("codex-app-server".into(), WorkspaceWrite)])
        );
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_newer_builds_unknown_default_falls_back_instead_of_failing() {
        let directory = std::env::temp_dir().join(format!("sage-access-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.sqlite");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE binding_access_modes (binding_id TEXT PRIMARY KEY, mode TEXT NOT NULL) STRICT;
                 INSERT INTO binding_access_modes VALUES ('claude-agent-acp','futureMode');
                 INSERT INTO binding_access_modes VALUES ('codex-app-server','workspaceWrite');",
            )
            .unwrap();
        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(
            store.session_access_modes().unwrap(),
            BTreeMap::from([("codex-app-server".into(), WorkspaceWrite)])
        );
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn access_modes_persist_per_binding_without_cross_harness_escalation() {
        let directory = std::env::temp_dir().join(format!("sage-access-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.sqlite");
        let store = SettingsStore::open(&path).unwrap();
        assert!(store.session_access_modes().unwrap().is_empty());
        let modes = BTreeMap::from([
            ("codex-app-server".into(), WorkspaceWrite),
            ("claude-agent-acp".into(), Ask),
            ("opencode-acp".into(), ReadOnly),
        ]);
        store.set_session_access_modes(&modes).unwrap();
        drop(store);
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .session_access_modes()
                .unwrap(),
            modes
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_global_choice_is_claimed_once_by_its_restored_binding_only() {
        let store = SettingsStore::in_memory().unwrap();
        store
            .0
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO session_access_mode VALUES (1,'workspaceWrite')",
                [],
            )
            .unwrap();
        assert!(store.pending_access_migration().unwrap());
        assert!(store.session_access_modes().is_err());
        let expected = BTreeMap::from([("codex-app-server".into(), WorkspaceWrite)]);
        assert_eq!(
            store
                .migrate_session_access_modes(Some("codex-app-server"))
                .unwrap(),
            expected
        );
        assert!(!store.pending_access_migration().unwrap());
        assert_eq!(
            store
                .migrate_session_access_modes(Some("claude-agent-acp"))
                .unwrap(),
            expected
        );
    }

    #[test]
    fn migration_does_not_overwrite_an_existing_scoped_choice() {
        let store = SettingsStore::in_memory().unwrap();
        store
            .set_session_access_modes(&BTreeMap::from([("codex-app-server".into(), ReadOnly)]))
            .unwrap();
        store
            .0
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO session_access_mode VALUES (1,'workspaceWrite')",
                [],
            )
            .unwrap();
        assert_eq!(
            store
                .migrate_session_access_modes(Some("codex-app-server"))
                .unwrap()["codex-app-server"],
            ReadOnly
        );
    }

    #[test]
    fn legacy_explicit_read_only_remains_scoped_and_never_escalates_to_claude_ask() {
        let store = SettingsStore::in_memory().unwrap();
        store
            .0
            .lock()
            .unwrap()
            .execute("INSERT INTO session_access_mode VALUES (1,'readOnly')", [])
            .unwrap();
        assert_eq!(
            store
                .migrate_session_access_modes(Some("claude-agent-acp"))
                .unwrap()["claude-agent-acp"],
            ReadOnly
        );
        assert!(!store.pending_access_migration().unwrap());
    }

    #[test]
    fn access_preferences_are_bounded() {
        let store = SettingsStore::in_memory().unwrap();
        let modes = (0..9).map(|index| (index.to_string(), ReadOnly)).collect();
        assert!(store.set_session_access_modes(&modes).is_err());
        assert!(store.session_access_modes().unwrap().is_empty());
    }
}
