use rusqlite::Connection;
use sage_core::error::{AppError, AppResult};

use super::{MAX_COLUMNS, MAX_SCHEMA_OBJECTS, MAX_SCHEMA_VERSION, SchemaClass, invalid, limit};

/// Admit Sage's INTEGER rowid alias before MAX or the classifier's ordered
/// LIMIT. Other primary keys can have an ordering that does not match the
/// version column, forcing a scan even though they are indexed.
pub(super) fn reject_newer_history(connection: &Connection) -> AppResult<()> {
    admit_history_index(connection)?;
    let newest: Option<i64> = connection.query_row(
        "SELECT MAX(version COLLATE BINARY) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if let Some(stored_version) = newest {
        reject_version(stored_version)?;
    }
    Ok(())
}

fn admit_history_index(connection: &Connection) -> AppResult<()> {
    let ordinary_table: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_list
           WHERE schema='main' AND name='schema_migrations' AND type='table' AND wr=0
         )",
        [],
        |row| row.get(0),
    )?;
    if !ordinary_table {
        return limit("migration history requires an ordinary rowid table");
    }
    let mut statement = connection
        .prepare("SELECT name, type, pk FROM pragma_table_xinfo('schema_migrations') LIMIT ?1")?;
    let mut rows = statement.query([MAX_COLUMNS as i64 + 1])?;
    let mut columns = 0;
    let mut version_key = false;
    while let Some(row) = rows.next()? {
        columns += 1;
        if columns > MAX_COLUMNS {
            return limit("migration history exceeds the 128-column bound");
        }
        let primary_key: i64 = row.get(2)?;
        if primary_key == 0 {
            continue;
        }
        let name: String = row.get(0)?;
        let declared_type: String = row.get(1)?;
        if primary_key != 1
            || !name.eq_ignore_ascii_case("version")
            || !declared_type.eq_ignore_ascii_case("INTEGER")
        {
            return limit("migration history requires a sole INTEGER primary key on version");
        }
        version_key = true;
    }
    if !version_key {
        return limit("migration history requires an indexed INTEGER primary key on version");
    }
    // An INTEGER primary key with no separate primary-key index aliases the
    // rowid. In particular this excludes SQLite's inline PRIMARY KEY DESC
    // exception. Bound metadata inspection as well as history inspection.
    let (indexes, separate_primary): (i64, bool) = connection.query_row(
        "SELECT COUNT(*), COALESCE(MAX(origin='pk'), 0) FROM (
           SELECT origin FROM pragma_index_list('schema_migrations') LIMIT ?1
         )",
        [MAX_SCHEMA_OBJECTS as i64 + 1],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if indexes > MAX_SCHEMA_OBJECTS as i64 || separate_primary {
        return limit("migration history requires version to alias the INTEGER rowid");
    }
    Ok(())
}

pub(super) fn reject_version(stored_version: i64) -> AppResult<()> {
    if stored_version > MAX_SCHEMA_VERSION as i64 {
        return Err(AppError::NewerProfile {
            stored_version,
            supported_version: MAX_SCHEMA_VERSION as u32,
        });
    }
    Ok(())
}

pub(super) fn class_for_version(version: u32) -> AppResult<SchemaClass> {
    Ok(match version {
        1 => SchemaClass::LegacyV1,
        2 => SchemaClass::LegacyV2,
        3 => SchemaClass::LegacyV3,
        4 => SchemaClass::LegacyV4,
        5 => SchemaClass::CurrentV5,
        6 => SchemaClass::CurrentV6,
        7 => SchemaClass::CurrentV7,
        8 => SchemaClass::CurrentV8,
        9 => SchemaClass::CurrentV9,
        10 => SchemaClass::CurrentV10,
        11 => SchemaClass::CurrentV11,
        12 => SchemaClass::CurrentV12,
        13 => SchemaClass::CurrentV13,
        14 => SchemaClass::CurrentV14,
        15 => SchemaClass::CurrentV15,
        16 => SchemaClass::CurrentV16,
        17 => SchemaClass::CurrentV17,
        18 => SchemaClass::CurrentV18,
        19 => SchemaClass::CurrentV19,
        20 => SchemaClass::CurrentV20,
        21 => SchemaClass::CurrentV21,
        22 => SchemaClass::CurrentV22,
        23 => SchemaClass::CurrentV23,
        version if version == super::super::session_resume_link::VERSION => {
            SchemaClass::CurrentResumeLink
        }
        version if version == super::super::title_source::VERSION => {
            SchemaClass::CurrentTitleSource
        }
        version if version == super::super::session_facts_turn_index::VERSION => {
            SchemaClass::CurrentSessionFactsTurnIndex
        }
        version if version == super::super::operation_terminal_index::VERSION => {
            SchemaClass::CurrentOperationTerminalIndex
        }
        version if version == super::super::turn_undo_intent_indexes::VERSION => {
            SchemaClass::CurrentTurnUndoIntentIndexes
        }
        version if version == super::super::imported_history::VERSION => {
            SchemaClass::CurrentImportedHistory
        }
        version if version == super::super::session_capabilities::VERSION => {
            SchemaClass::CurrentSessionCapabilities
        }
        _ => return invalid("Sage schema version is above the supported maximum"),
    })
}
