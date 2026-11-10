use std::path::Path;

use rusqlite::{Connection, types::ValueRef};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

mod expected;
mod version;
use expected::{expected_legacy_v10_session_events_fingerprint, expected_schema_fingerprint};
use version::class_for_version;

pub(super) use super::file_artifact::{FileFingerprint, file_fingerprint, open_read_only};

pub(super) const V10_LEGACY_SESSION_EVENTS: &str = "CREATE TABLE session_events (\n  id TEXT PRIMARY KEY,\n  session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,\n  sequence INTEGER NOT NULL,\n  kind TEXT NOT NULL,\n  payload_json TEXT NOT NULL,\n  created_at INTEGER NOT NULL,\n  UNIQUE (session_id, sequence)\n) STRICT;\nCREATE INDEX idx_session_events_order\n  ON session_events(session_id, sequence);";
// One sentinel row detects migration histories newer than this binary.
const MAX_SCHEMA_VERSION: usize = super::session_facts_turn_index::VERSION as usize;
const MAX_SCHEMA_OBJECTS: usize = 128;
const MAX_SCHEMA_BYTES: usize = 1_048_576;
const MAX_COLUMNS: usize = 128;
const MAX_LOGICAL_ROWS: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum SchemaClass {
    Fresh,
    LegacyV1,
    LegacyV2,
    LegacyV3,
    LegacyV4,
    CurrentV5,
    CurrentV6,
    CurrentV7,
    CurrentV8,
    CurrentV9,
    CurrentV10,
    CurrentV10LegacySessionEvents,
    CurrentV11,
    CurrentV12,
    CurrentV13,
    CurrentV14,
    CurrentV15,
    CurrentV16,
    CurrentV17,
    CurrentV18,
    CurrentV19,
    CurrentV20,
    CurrentV21,
    CurrentV22,
    CurrentV23,
    /// Schema v23 plus the session resume link; its version number lives in
    /// `session_resume_link::VERSION`.
    CurrentResumeLink,
    /// The resume link schema plus each chat title's source; its version
    /// number lives in `title_source::VERSION`.
    CurrentTitleSource,
    /// The title source schema plus the per-Turn session facts index; its
    /// version number lives in `session_facts_turn_index::VERSION`.
    CurrentSessionFactsTurnIndex,
}

/// The schema a fresh or fully migrated database reaches.
#[cfg(test)]
pub(super) const CURRENT_SCHEMA: SchemaClass = SchemaClass::CurrentSessionFactsTurnIndex;

/// The version number of [`CURRENT_SCHEMA`].
#[cfg(test)]
pub(crate) const CURRENT_SCHEMA_VERSION: u32 = super::session_facts_turn_index::VERSION;

impl SchemaClass {
    pub(super) fn is_legacy(self) -> bool {
        matches!(
            self,
            Self::LegacyV1 | Self::LegacyV2 | Self::LegacyV3 | Self::LegacyV4
        )
    }

    pub(super) fn requires_retained_backup(self) -> bool {
        matches!(
            self,
            Self::LegacyV1
                | Self::LegacyV2
                | Self::LegacyV3
                | Self::LegacyV4
                | Self::CurrentV5
                | Self::CurrentV6
                | Self::CurrentV7
                | Self::CurrentV8
                | Self::CurrentV9
                | Self::CurrentV10
                | Self::CurrentV10LegacySessionEvents
                | Self::CurrentV11
                | Self::CurrentV12
                | Self::CurrentV13
                | Self::CurrentV14
                | Self::CurrentV15
                | Self::CurrentV16
                | Self::CurrentV17
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DatabaseFingerprint {
    pub(super) schema: SchemaClass,
    pub(super) schema_sha256: String,
    pub(super) logical_sha256: String,
}

pub(super) fn classify_path(path: &Path) -> AppResult<SchemaClass> {
    super::platform::reject_path_alias(path)?;
    if !path.exists() || std::fs::metadata(path)?.len() == 0 {
        return Ok(SchemaClass::Fresh);
    }
    let connection = open_read_only(path)?;
    classify_connection(&connection)
}

pub(super) fn classify_connection(connection: &Connection) -> AppResult<SchemaClass> {
    let marker: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM sqlite_schema
           WHERE type='table' AND name NOT LIKE 'sqlite_%' LIMIT ?1
         )",
        [MAX_SCHEMA_OBJECTS as i64 + 1],
        |row| row.get(0),
    )?;
    let has_migrations: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM sqlite_schema WHERE type='table' AND name='schema_migrations'
         )",
        [],
        |row| row.get(0),
    )?;
    if !has_migrations {
        if marker == 0 && tables == 0 {
            return Ok(SchemaClass::Fresh);
        }
        return invalid("nonempty database has no exact Sage migration schema");
    }
    let versions = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version LIMIT ?1")?
        .query_map([MAX_SCHEMA_VERSION as i64 + 1], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions.is_empty()
        || versions.len() > MAX_SCHEMA_VERSION
        || versions
            .iter()
            .enumerate()
            .any(|(index, value)| *value != index as i64 + 1)
    {
        return invalid("Sage migration history is gapped or unsupported");
    }
    let version = versions.len() as u32;
    if marker != 0 && marker != i64::from(version) {
        return invalid("Sage user_version does not match migration history");
    }
    if version >= 5 && marker != i64::from(version) {
        return invalid("current schema requires both exact version markers");
    }
    let class = class_for_version(version)?;
    let actual = schema_fingerprint(connection)?;
    let expected = expected_schema_fingerprint(version)?;
    if actual == expected {
        return Ok(class);
    }
    if version == 10 && actual == expected_legacy_v10_session_events_fingerprint()? {
        return Ok(SchemaClass::CurrentV10LegacySessionEvents);
    }
    invalid("Sage schema objects do not match the exact declared version")
}

pub(super) fn database_fingerprint(connection: &Connection) -> AppResult<DatabaseFingerprint> {
    let schema = classify_connection(connection)?;
    if schema == SchemaClass::Fresh {
        return invalid("an empty database has no migration source fingerprint");
    }
    Ok(DatabaseFingerprint {
        schema,
        schema_sha256: schema_fingerprint(connection)?,
        logical_sha256: logical_fingerprint(connection)?,
    })
}

fn schema_fingerprint(connection: &Connection) -> AppResult<String> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, COALESCE(sql, '')
         FROM sqlite_schema
         WHERE name NOT LIKE 'sqlite_%'
         ORDER BY type, name, tbl_name
         LIMIT ?1",
    )?;
    let mut rows = statement.query([MAX_SCHEMA_OBJECTS as i64 + 1])?;
    let mut hash = Sha256::new();
    let mut count = 0_usize;
    let mut bytes = 0_usize;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > MAX_SCHEMA_OBJECTS {
            return limit("schema contains too many objects");
        }
        for index in 0..4 {
            let value: String = row.get(index)?;
            bytes = bytes.saturating_add(value.len());
            if bytes > MAX_SCHEMA_BYTES {
                return limit("schema text exceeds 1 MiB");
            }
            update_bytes(&mut hash, value.as_bytes());
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn logical_fingerprint(connection: &Connection) -> AppResult<String> {
    let table_names = connection
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type='table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name LIMIT ?1",
        )?
        .query_map([MAX_SCHEMA_OBJECTS as i64 + 1], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if table_names.len() > MAX_SCHEMA_OBJECTS {
        return limit("logical fingerprint has too many tables");
    }
    let mut hash = Sha256::new();
    let mut total_rows = 0_usize;
    for table in table_names {
        update_bytes(&mut hash, table.as_bytes());
        let quoted = quote_identifier(&table);
        let columns = table_columns(connection, &quoted)?;
        let order = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT * FROM {quoted} ORDER BY {order}");
        let mut statement = connection.prepare(&query)?;
        let column_count = statement.column_count();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            total_rows += 1;
            if total_rows > MAX_LOGICAL_ROWS {
                return limit("logical fingerprint exceeds 200,000 rows");
            }
            hash.update([0xff]);
            for index in 0..column_count {
                update_value(&mut hash, row.get_ref(index)?);
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn table_columns(connection: &Connection, quoted_table: &str) -> AppResult<Vec<String>> {
    let table = quoted_table.trim_matches('"').replace("\"\"", "\"");
    let columns = connection
        .prepare("SELECT name FROM pragma_table_xinfo(?1) LIMIT ?2")?
        .query_map(rusqlite::params![table, MAX_COLUMNS as i64 + 1], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() || columns.len() > MAX_COLUMNS {
        return invalid("table has no columns or exceeds the 128-column bound");
    }
    Ok(columns)
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn update_value(hash: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => hash.update([0]),
        ValueRef::Integer(value) => {
            hash.update([1]);
            hash.update(value.to_be_bytes());
        }
        ValueRef::Real(value) => {
            hash.update([2]);
            hash.update(value.to_bits().to_be_bytes());
        }
        ValueRef::Text(value) => {
            hash.update([3]);
            update_bytes(hash, value);
        }
        ValueRef::Blob(value) => {
            hash.update([4]);
            update_bytes(hash, value);
        }
    }
}

fn update_bytes(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}

fn limit<T>(message: &str) -> AppResult<T> {
    Err(AppError::ResourceLimit(message.into()))
}
