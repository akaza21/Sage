//! The durable defaults a new chat starts from: a harness, and optionally a
//! model and an effort within it. One closed record. The store only admits
//! well-formed identifiers and keeps them exactly as given; judging them
//! against discovery (the settings commands) and against a live session's
//! configuration (new chat start) belongs to the callers, which report a value
//! they cannot use instead of rewriting it.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::{AppError, AppResult};
use sage_db::error::SqliteResultExt;
use serde::{Deserialize, Serialize};

use super::SettingsStore;

/// The longest harness, model or effort identifier the store keeps.
pub const MAX_NEW_CHAT_DEFAULT_BYTES: usize = 256;

/// Model and effort belong to the harness they were chosen in, so the record
/// is replaced whole: saving a new harness never keeps a model or effort that
/// was picked for another one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewChatDefaults {
    #[serde(default)]
    pub harness_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS new_chat_defaults (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           harness_id TEXT NOT NULL
             CHECK (length(CAST(harness_id AS BLOB)) BETWEEN 1 AND 256),
           model_id TEXT
             CHECK (model_id IS NULL OR length(CAST(model_id AS BLOB)) BETWEEN 1 AND 256),
           effort TEXT
             CHECK (effort IS NULL OR length(CAST(effort AS BLOB)) BETWEEN 1 AND 256)
         ) STRICT;",
        )
        .db()?;
    Ok(())
}

fn admitted_value(field: &str, value: Option<&str>) -> AppResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty()
        || value.len() > MAX_NEW_CHAT_DEFAULT_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "new chat default {field} is not a valid identifier"
        )));
    }
    Ok(())
}

pub(crate) fn admitted_new_chat_defaults(value: &NewChatDefaults) -> AppResult<()> {
    admitted_value("harness", value.harness_id.as_deref())?;
    admitted_value("model", value.model_id.as_deref())?;
    admitted_value("effort", value.effort.as_deref())?;
    if value.harness_id.is_none() && (value.model_id.is_some() || value.effort.is_some()) {
        return Err(AppError::InvalidRequest(
            "a new chat default model or effort needs its harness".into(),
        ));
    }
    Ok(())
}

impl SettingsStore {
    pub fn new_chat_defaults(&self) -> AppResult<NewChatDefaults> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stored = connection
            .query_row(
                "SELECT harness_id, model_id, effort FROM new_chat_defaults WHERE singleton = 1",
                [],
                |row| {
                    Ok(NewChatDefaults {
                        harness_id: Some(row.get(0)?),
                        model_id: row.get(1)?,
                        effort: row.get(2)?,
                    })
                },
            )
            .optional()
            .db()?;
        Ok(stored.unwrap_or_default())
    }

    /// Replaces the whole record; a record without a harness clears it.
    pub fn set_new_chat_defaults(&self, value: NewChatDefaults) -> AppResult<NewChatDefaults> {
        admitted_new_chat_defaults(&value)?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match value.harness_id.as_deref() {
            None => {
                connection
                    .execute("DELETE FROM new_chat_defaults WHERE singleton = 1", [])
                    .db()?;
            }
            Some(harness_id) => {
                connection
                    .execute(
                        "INSERT INTO new_chat_defaults(singleton, harness_id, model_id, effort)
                     VALUES (1, ?1, ?2, ?3)
                     ON CONFLICT(singleton) DO UPDATE SET
                       harness_id = excluded.harness_id,
                       model_id = excluded.model_id,
                       effort = excluded.effort",
                        params![harness_id, value.model_id, value.effort],
                    )
                    .db()?;
            }
        }
        Ok(value)
    }
}

#[cfg(test)]
#[path = "new_chat_defaults_tests.rs"]
mod tests;
