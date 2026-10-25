//! The editor settings catalogue (EDIT-0005 AD-2). One closed registry lists
//! each key with its type, default, range and owning ticket, and the values
//! live as versioned key/value rows. `src/domain/editorSettingsCatalogue.ts`
//! mirrors the registry, and a parity test keeps the two equal, so adding a
//! setting is one entry on each side and never a schema change.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{Mutex, OnceLock},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::SettingsStore;
use crate::error::{AppError, AppResult};

/// The row format version. A later format bumps it and converts older rows.
const ROW_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
}

#[derive(Debug, Clone, Copy)]
pub enum SettingKind {
    Bool,
    /// A whole number in `min..=max`; when `choices` is not empty, only those
    /// values are allowed.
    Int {
        min: i64,
        max: i64,
        choices: &'static [i64],
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    pub key: &'static str,
    pub kind: SettingKind,
    pub default: SettingValue,
    /// The ticket that owns the setting. Only tests and the TypeScript mirror
    /// read it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "documents ownership for the parity test")
    )]
    pub ticket: &'static str,
}

pub const EDITOR_SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "editor.fontSize",
        kind: SettingKind::Int {
            min: 12,
            max: 16,
            choices: &[12, 13, 14, 16],
        },
        default: SettingValue::Int(13),
        ticket: "EP-02",
    },
    SettingSpec {
        key: "editor.lineNumbers",
        kind: SettingKind::Bool,
        default: SettingValue::Bool(true),
        ticket: "EP-02",
    },
    SettingSpec {
        key: "editor.wordWrap",
        kind: SettingKind::Bool,
        default: SettingValue::Bool(false),
        ticket: "EP-02",
    },
    SettingSpec {
        key: "editor.renderWhitespace",
        kind: SettingKind::Bool,
        default: SettingValue::Bool(false),
        ticket: "EP-02",
    },
    SettingSpec {
        key: "editor.tabSize",
        kind: SettingKind::Int {
            min: 2,
            max: 8,
            choices: &[2, 4, 8],
        },
        default: SettingValue::Int(2),
        ticket: "EP-02",
    },
];

/// Every registry key with its current value; keys never go missing.
pub type EditorSettings = BTreeMap<&'static str, SettingValue>;

fn spec(key: &str) -> Option<&'static SettingSpec> {
    EDITOR_SETTINGS.iter().find(|spec| spec.key == key)
}

fn defaults() -> EditorSettings {
    EDITOR_SETTINGS
        .iter()
        .map(|spec| (spec.key, spec.default))
        .collect()
}

/// The value as the registry allows it: whole numbers are clamped to the
/// range, then moved to the nearest allowed choice (the lower one on a tie).
/// `None` means the value has the wrong type.
fn admit(spec: &SettingSpec, value: &Value) -> Option<SettingValue> {
    match spec.kind {
        SettingKind::Bool => value.as_bool().map(SettingValue::Bool),
        SettingKind::Int { min, max, choices } => safe_integer(value).map(|number| {
            let clamped = number.clamp(min, max);
            SettingValue::Int(
                choices
                    .iter()
                    .copied()
                    .min_by_key(|choice| ((choice - clamped).abs(), *choice))
                    .unwrap_or(clamped),
            )
        }),
    }
}

/// A JSON number that is a whole number within JavaScript's safe-integer
/// range, so Rust and the TypeScript catalogue admit the same values.
fn safe_integer(value: &Value) -> Option<i64> {
    const MAX_SAFE: i64 = (1 << 53) - 1;
    let number = value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.fract() == 0.0 && number.abs() <= MAX_SAFE as f64)
            .map(|number| number as i64)
    })?;
    (number.abs() <= MAX_SAFE).then_some(number)
}

/// Logs a problem with a stored or requested key once per process, so a
/// stale row from a newer Sage does not repeat on every read.
fn note_once(problem: &'static str, key: &str) {
    static NOTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let first = NOTED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(format!("{problem}:{key}"));
    if first {
        tracing::warn!(key, "editor setting ignored: {problem}");
    }
}

/// Creates the rows table and, the first time, copies the five values from
/// the legacy `editor_appearance` columns. Runs inside the store's opening
/// transaction, after those columns are brought up to date.
pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS editor_settings (
           key TEXT PRIMARY KEY CHECK (length(key) BETWEEN 1 AND 128),
           value TEXT NOT NULL CHECK (json_valid(value)),
           version INTEGER NOT NULL CHECK (version >= 1)
         ) STRICT;",
    )?;
    let empty: bool = connection.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM editor_settings)",
        [],
        |row| row.get(0),
    )?;
    if !empty {
        return Ok(());
    }
    let legacy = connection
        .query_row(
            "SELECT font_size, tab_size, word_wrap, line_numbers, show_whitespace
             FROM editor_appearance WHERE singleton = 1",
            [],
            |row| {
                Ok([
                    ("editor.fontSize", Value::from(row.get::<_, i64>(0)?)),
                    ("editor.tabSize", Value::from(row.get::<_, i64>(1)?)),
                    ("editor.wordWrap", Value::from(row.get::<_, bool>(2)?)),
                    ("editor.lineNumbers", Value::from(row.get::<_, bool>(3)?)),
                    (
                        "editor.renderWhitespace",
                        Value::from(row.get::<_, bool>(4)?),
                    ),
                ])
            },
        )
        .optional()?;
    for (key, value) in legacy.into_iter().flatten() {
        upsert(connection, key, &value)?;
    }
    Ok(())
}

fn json(value: SettingValue) -> Value {
    match value {
        SettingValue::Bool(value) => Value::from(value),
        SettingValue::Int(value) => Value::from(value),
    }
}

fn upsert(connection: &Connection, key: &str, value: &Value) -> AppResult<()> {
    connection.execute(
        "INSERT INTO editor_settings(key, value, version) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, version = excluded.version",
        params![key, value.to_string(), ROW_VERSION],
    )?;
    Ok(())
}

fn read(connection: &Connection) -> AppResult<EditorSettings> {
    let mut settings = defaults();
    let mut statement = connection.prepare("SELECT key, value FROM editor_settings")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (key, raw) = row?;
        let Some(spec) = spec(&key) else {
            note_once("unknown key", &key);
            continue;
        };
        match serde_json::from_str(&raw)
            .ok()
            .and_then(|value| admit(spec, &value))
        {
            Some(value) => {
                settings.insert(spec.key, value);
            }
            None => note_once("stored value has the wrong type", &key),
        }
    }
    Ok(settings)
}

impl SettingsStore {
    pub fn editor_settings(&self) -> AppResult<EditorSettings> {
        read(
            &self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Stores the given keys and returns every value. Unknown keys are
    /// ignored and logged once; out-of-range numbers are clamped; a value of
    /// the wrong type rejects the whole change.
    pub fn set_editor_settings(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> AppResult<EditorSettings> {
        let mut admitted = Vec::new();
        for (key, value) in changes {
            let Some(spec) = spec(key) else {
                note_once("unknown key", key);
                continue;
            };
            let value = admit(spec, value).ok_or_else(|| {
                AppError::InvalidRequest(format!("editor setting {key} has the wrong type"))
            })?;
            admitted.push((spec.key, value));
        }
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        for (key, value) in admitted {
            upsert(&transaction, key, &json(value))?;
        }
        transaction.commit()?;
        read(&connection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_store_path(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("sage-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("settings.db")
    }

    fn changes(entries: serde_json::Value) -> BTreeMap<String, Value> {
        serde_json::from_value(entries).unwrap()
    }

    fn legacy_store(path: &std::path::Path, schema_and_row: &str) {
        Connection::open(path)
            .unwrap()
            .execute_batch(schema_and_row)
            .unwrap();
    }

    #[test]
    fn every_entry_is_unique_owned_and_admits_its_default() {
        let mut keys = HashSet::new();
        for spec in EDITOR_SETTINGS {
            assert!(keys.insert(spec.key), "duplicate key {}", spec.key);
            assert!(spec.key.starts_with("editor."), "{}", spec.key);
            assert!(
                spec.ticket.len() == 5 && spec.ticket.starts_with("EP-"),
                "{} has no owning ticket",
                spec.key
            );
            assert_eq!(admit(spec, &json(spec.default)), Some(spec.default));
        }
    }

    #[test]
    fn a_fresh_store_reads_every_registry_default() {
        let store = SettingsStore::in_memory().unwrap();
        let settings = store.editor_settings().unwrap();
        assert_eq!(settings.len(), EDITOR_SETTINGS.len());
        for spec in EDITOR_SETTINGS {
            assert_eq!(settings[spec.key], spec.default, "{}", spec.key);
        }
    }

    #[test]
    fn migrates_the_five_legacy_columns_without_loss() {
        let path = temp_store_path("settings-five");
        legacy_store(
            &path,
            "CREATE TABLE editor_appearance (
               singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
               font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
               tab_size INTEGER NOT NULL CHECK (tab_size IN (2, 4, 8)),
               word_wrap INTEGER NOT NULL CHECK (word_wrap IN (0, 1)),
               line_numbers INTEGER NOT NULL DEFAULT 1 CHECK (line_numbers IN (0, 1)),
               show_whitespace INTEGER NOT NULL DEFAULT 0 CHECK (show_whitespace IN (0, 1))
             ) STRICT;
             INSERT INTO editor_appearance VALUES (1, 16, 8, 1, 0, 1);",
        );
        let expected = EditorSettings::from([
            ("editor.fontSize", SettingValue::Int(16)),
            ("editor.lineNumbers", SettingValue::Bool(false)),
            ("editor.renderWhitespace", SettingValue::Bool(true)),
            ("editor.tabSize", SettingValue::Int(8)),
            ("editor.wordWrap", SettingValue::Bool(true)),
        ]);
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .editor_settings()
                .unwrap(),
            expected
        );
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .editor_settings()
                .unwrap(),
            expected
        );
        let connection = Connection::open(&path).unwrap();
        let versions: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM editor_settings WHERE version = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(versions, 5);
        drop(connection);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn migrates_the_three_column_record_with_display_defaults() {
        let path = temp_store_path("settings-three");
        legacy_store(
            &path,
            "CREATE TABLE editor_appearance (
               singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
               font_size INTEGER NOT NULL CHECK (font_size IN (12, 13, 14, 16)),
               tab_size INTEGER NOT NULL CHECK (tab_size IN (2, 4, 8)),
               word_wrap INTEGER NOT NULL CHECK (word_wrap IN (0, 1))
             ) STRICT;
             INSERT INTO editor_appearance(singleton, font_size, tab_size, word_wrap)
             VALUES (1, 14, 4, 1);",
        );
        let settings = SettingsStore::open(&path)
            .unwrap()
            .editor_settings()
            .unwrap();
        assert_eq!(settings["editor.fontSize"], SettingValue::Int(14));
        assert_eq!(settings["editor.tabSize"], SettingValue::Int(4));
        assert_eq!(settings["editor.wordWrap"], SettingValue::Bool(true));
        assert_eq!(settings["editor.lineNumbers"], SettingValue::Bool(true));
        assert_eq!(
            settings["editor.renderWhitespace"],
            SettingValue::Bool(false)
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn copies_the_legacy_record_only_once() {
        let path = temp_store_path("settings-once");
        let store = SettingsStore::open(&path).unwrap();
        store
            .set_editor_settings(&changes(json!({ "editor.fontSize": 14 })))
            .unwrap();
        drop(store);
        legacy_store(
            &path,
            "INSERT INTO editor_appearance(singleton, font_size, tab_size, word_wrap)
             VALUES (1, 16, 8, 1);",
        );
        let settings = SettingsStore::open(&path)
            .unwrap()
            .editor_settings()
            .unwrap();
        assert_eq!(settings["editor.fontSize"], SettingValue::Int(14));
        assert_eq!(settings["editor.tabSize"], SettingValue::Int(2));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn keeps_values_across_a_reopen() {
        let path = temp_store_path("settings-reopen");
        SettingsStore::open(&path)
            .unwrap()
            .set_editor_settings(&changes(
                json!({ "editor.tabSize": 8, "editor.renderWhitespace": true }),
            ))
            .unwrap();
        let settings = SettingsStore::open(&path)
            .unwrap()
            .editor_settings()
            .unwrap();
        assert_eq!(settings["editor.tabSize"], SettingValue::Int(8));
        assert_eq!(
            settings["editor.renderWhitespace"],
            SettingValue::Bool(true)
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn clamps_numbers_to_the_range_and_the_nearest_choice() {
        let store = SettingsStore::in_memory().unwrap();
        for (requested, stored) in [(15, 14), (99, 16), (-3, 12), (13, 13)] {
            let settings = store
                .set_editor_settings(&changes(json!({ "editor.fontSize": requested })))
                .unwrap();
            assert_eq!(settings["editor.fontSize"], SettingValue::Int(stored));
        }
        for (requested, stored) in [(3, 2), (6, 4), (7, 8), (100, 8)] {
            let settings = store
                .set_editor_settings(&changes(json!({ "editor.tabSize": requested })))
                .unwrap();
            assert_eq!(settings["editor.tabSize"], SettingValue::Int(stored));
        }
        let settings = store
            .set_editor_settings(&changes(json!({ "editor.fontSize": 14.0 })))
            .unwrap();
        assert_eq!(settings["editor.fontSize"], SettingValue::Int(14));
        for unsafe_number in [json!(9_007_199_254_740_992_i64), json!(13.5)] {
            assert!(
                store
                    .set_editor_settings(&changes(json!({ "editor.fontSize": unsafe_number })))
                    .is_err()
            );
        }
    }

    #[test]
    fn ignores_unknown_keys_when_writing_and_reading() {
        let store = SettingsStore::in_memory().unwrap();
        let settings = store
            .set_editor_settings(&changes(
                json!({ "editor.unknown": 1, "editor.wordWrap": true }),
            ))
            .unwrap();
        assert!(!settings.contains_key("editor.unknown"));
        assert_eq!(settings["editor.wordWrap"], SettingValue::Bool(true));
        upsert(
            &store.0.lock().unwrap(),
            "editor.fromANewerSage",
            &json!("value"),
        )
        .unwrap();
        let mut expected = defaults();
        expected.insert("editor.wordWrap", SettingValue::Bool(true));
        assert_eq!(store.editor_settings().unwrap(), expected);
    }

    #[test]
    fn rejects_a_write_of_the_wrong_type_and_reads_a_bad_row_as_its_default() {
        let store = SettingsStore::in_memory().unwrap();
        assert!(
            store
                .set_editor_settings(&changes(
                    json!({ "editor.wordWrap": true, "editor.fontSize": "large" }),
                ))
                .is_err()
        );
        assert_eq!(
            store.editor_settings().unwrap()["editor.wordWrap"],
            SettingValue::Bool(false)
        );
        upsert(&store.0.lock().unwrap(), "editor.tabSize", &json!(true)).unwrap();
        assert_eq!(
            store.editor_settings().unwrap()["editor.tabSize"],
            SettingValue::Int(2)
        );
    }

    #[test]
    fn serializes_values_as_plain_json() {
        let settings = SettingsStore::in_memory()
            .unwrap()
            .editor_settings()
            .unwrap();
        assert_eq!(
            serde_json::to_value(settings).unwrap(),
            json!({
                "editor.fontSize": 13,
                "editor.lineNumbers": true,
                "editor.renderWhitespace": false,
                "editor.tabSize": 2,
                "editor.wordWrap": false,
            })
        );
    }
}
