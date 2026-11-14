use std::{fs, path::Path};

use rusqlite::{Connection, params};

use sage_core::error::{AppError, AppResult};

use super::fingerprint::SchemaClass;

pub(super) const ROWS_PER_PAGE: usize = 256;
pub(super) const BYTES_PER_PAGE: usize = 1_048_576;
pub(super) const ITEM_BYTES: usize = 262_144;
pub(super) const ITEMS_PER_TURN: usize = 256;
pub(super) const BYTES_PER_TURN: usize = 4_194_304;
pub(super) const MANIFEST_BYTES: u64 = 65_536;
pub(super) const LIVE_SQLITE_BYTES: u64 = 1_073_741_824;
pub(super) const TEMP_SQLITE_BYTES: u64 = 1_073_741_824;
pub(super) const WAL_HARD_BYTES: u64 = 33_554_432;
pub(super) const JOURNAL_BYTES: u64 = 8_388_608;
pub(super) const SIDE_FILES_BYTES: u64 = 1_048_576;
pub(super) const MIGRATION_FREE_BYTES: u64 = 2_254_857_831;

const PROJECTS: i64 = 64;
const ROOTS: i64 = 512;
const ROOTS_PER_PROJECT: i64 = 8;
const TABS: i64 = 4_096;
const TABS_PER_PROJECT: i64 = 64;
const BUFFERS: i64 = 1_024;
const BUFFERS_PER_PROJECT: i64 = 16;
const SESSIONS: i64 = 4_096;
const SESSIONS_PER_CHAT: i64 = 32;
const EVENTS: i64 = 50_000;
const EVENTS_PER_SESSION: i64 = 5_000;
const SESSION_METADATA_BYTES: i64 = 16_777_216;
const SESSION_PAYLOAD_BYTES: i64 = 67_108_864;
const GLOBAL_PAYLOAD_BYTES: i64 = 268_435_456;

#[derive(Clone, Copy)]
pub(super) struct PreflightBounds {
    pub(super) projects: i64,
    pub(super) roots: i64,
    pub(super) roots_per_project: i64,
    pub(super) tabs: i64,
    pub(super) tabs_per_project: i64,
    pub(super) buffers: i64,
    pub(super) buffers_per_project: i64,
    pub(super) events: i64,
    pub(super) events_per_session: i64,
    pub(super) session_metadata_bytes: i64,
    pub(super) payload_bytes: i64,
}

pub(super) const PRODUCTION_BOUNDS: PreflightBounds = PreflightBounds {
    projects: PROJECTS,
    roots: ROOTS,
    roots_per_project: ROOTS_PER_PROJECT,
    tabs: TABS,
    tabs_per_project: TABS_PER_PROJECT,
    buffers: BUFFERS,
    buffers_per_project: BUFFERS_PER_PROJECT,
    events: EVENTS,
    events_per_session: EVENTS_PER_SESSION,
    session_metadata_bytes: SESSION_METADATA_BYTES,
    payload_bytes: GLOBAL_PAYLOAD_BYTES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LegacyTotals {
    pub(super) projects: i64,
    pub(super) roots: i64,
    pub(super) tabs: i64,
    pub(super) buffers: i64,
    pub(super) sessions: i64,
    pub(super) events: i64,
    pub(super) session_metadata_bytes: i64,
    pub(super) payload_bytes: i64,
}

pub(super) fn preflight(connection: &Connection, live: &Path) -> AppResult<LegacyTotals> {
    preflight_with_bounds(connection, live, PRODUCTION_BOUNDS)
}

pub(super) fn preflight_original_schema(
    connection: &Connection,
    live: &Path,
    schema: SchemaClass,
) -> AppResult<()> {
    if schema.is_legacy() {
        preflight(connection, live)?;
    } else if matches!(
        schema,
        SchemaClass::CurrentV5
            | SchemaClass::CurrentV6
            | SchemaClass::CurrentV7
            | SchemaClass::CurrentV8
            | SchemaClass::CurrentV9
            | SchemaClass::CurrentV10
            | SchemaClass::CurrentV11
            | SchemaClass::CurrentV12
            | SchemaClass::CurrentV13
            | SchemaClass::CurrentV14
            | SchemaClass::CurrentV15
            | SchemaClass::CurrentV16
            | SchemaClass::CurrentV17
    ) {
        verify_file_envelope(live)?;
    } else if schema == SchemaClass::CurrentV10LegacySessionEvents {
        verify_file_envelope(live)?;
        let has_events: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_events LIMIT 1)",
            [],
            |row| row.get(0),
        )?;
        if has_events {
            return Err(AppError::InvalidRequest(
                "historical v10 session events must be empty before migration".into(),
            ));
        }
    } else {
        return Err(AppError::InvalidRequest(
            "backup source does not require an original-schema backup".into(),
        ));
    }
    Ok(())
}

pub(super) fn preflight_with_bounds(
    connection: &Connection,
    live: &Path,
    bounds: PreflightBounds,
) -> AppResult<LegacyTotals> {
    verify_file_envelope(live)?;
    let projects = bounded_count(connection, "projects", bounds.projects)?;
    let roots = bounded_count(connection, "project_roots", bounds.roots)?;
    let tabs = bounded_count(connection, "workbench_tabs", bounds.tabs)?;
    let buffers = if table_exists(connection, "editor_buffers")? {
        bounded_count(connection, "editor_buffers", bounds.buffers)?
    } else {
        0
    };
    let sessions = bounded_count(connection, "agent_sessions", SESSIONS)?;
    let events = bounded_count(connection, "session_events", bounds.events)?;

    bounded_group(
        connection,
        "project_roots",
        "project_id",
        bounds.roots_per_project,
    )?;
    bounded_group(
        connection,
        "workbench_tabs",
        "project_id",
        bounds.tabs_per_project,
    )?;
    if buffers > 0 {
        bounded_group(
            connection,
            "editor_buffers",
            "project_id",
            bounds.buffers_per_project,
        )?;
    }
    bounded_group(
        connection,
        "agent_sessions",
        "chat_tab_id",
        SESSIONS_PER_CHAT,
    )?;
    bounded_group(
        connection,
        "session_events",
        "session_id",
        bounds.events_per_session,
    )?;

    let session_metadata_bytes = aggregate_session_metadata(connection)?;
    if session_metadata_bytes > bounds.session_metadata_bytes {
        return limit("legacy session metadata exceeds 16 MiB");
    }
    let payload_bytes = aggregate_payload_bytes(connection)?;
    if payload_bytes > bounds.payload_bytes {
        return limit("legacy payloads exceed 256 MiB");
    }
    let oversized_session: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM session_events
           GROUP BY session_id
           HAVING SUM(length(CAST(payload_json AS BLOB))) > ?1
           LIMIT 1
         )",
        [SESSION_PAYLOAD_BYTES],
        |row| row.get(0),
    )?;
    if oversized_session {
        return limit("a legacy session payload exceeds 64 MiB");
    }
    let invalid_event: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM session_events
           WHERE length(CAST(id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(payload_json AS BLOB)) > ?1
           LIMIT 1
         )",
        [ITEM_BYTES as i64],
        |row| row.get(0),
    )?;
    if invalid_event {
        return limit("a legacy event violates its ID or 256 KiB payload bound");
    }
    verify_legacy_ownership(connection)?;
    super::legacy_project::preflight(connection)?;

    Ok(LegacyTotals {
        projects,
        roots,
        tabs,
        buffers,
        sessions,
        events,
        session_metadata_bytes,
        payload_bytes,
    })
}

fn bounded_count(connection: &Connection, table: &str, limit_value: i64) -> AppResult<i64> {
    let query = format!("SELECT COUNT(*) FROM (SELECT 1 FROM {table} LIMIT ?1)");
    let count = connection.query_row(&query, [limit_value + 1], |row| row.get(0))?;
    if count > limit_value {
        return limit(&format!(
            "legacy {table} exceeds its {limit_value}-row limit"
        ));
    }
    Ok(count)
}

fn bounded_group(
    connection: &Connection,
    table: &str,
    owner: &str,
    limit_value: i64,
) -> AppResult<()> {
    let query = format!(
        "SELECT EXISTS(
           SELECT 1 FROM {table} GROUP BY {owner} HAVING COUNT(*) > ?1 LIMIT 1
         )"
    );
    let exceeded: bool = connection.query_row(&query, [limit_value], |row| row.get(0))?;
    if exceeded {
        return limit(&format!(
            "legacy {table} exceeds its {limit_value}-row per-owner limit"
        ));
    }
    Ok(())
}

fn aggregate_session_metadata(connection: &Connection) -> AppResult<i64> {
    Ok(connection.query_row(
        "SELECT COALESCE(SUM(
           length(CAST(id AS BLOB)) + length(CAST(project_id AS BLOB)) +
           length(CAST(chat_tab_id AS BLOB)) + length(CAST(harness_id AS BLOB)) +
           COALESCE(length(CAST(harness_version AS BLOB)), 0) +
           COALESCE(length(CAST(model_id AS BLOB)), 0) +
           COALESCE(length(CAST(external_session_id AS BLOB)), 0)
         ), 0)
         FROM (SELECT * FROM agent_sessions LIMIT ?1)",
        [SESSIONS + 1],
        |row| row.get(0),
    )?)
}

fn aggregate_payload_bytes(connection: &Connection) -> AppResult<i64> {
    Ok(connection.query_row(
        "SELECT COALESCE(SUM(length(CAST(payload_json AS BLOB))), 0)
         FROM (SELECT payload_json FROM session_events LIMIT ?1)",
        [EVENTS + 1],
        |row| row.get(0),
    )?)
}

fn verify_legacy_ownership(connection: &Connection) -> AppResult<()> {
    let invalid: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1
           FROM agent_sessions s
           LEFT JOIN workbench_tabs t ON t.id = s.chat_tab_id
           WHERE t.id IS NULL OR t.kind <> 'chat' OR t.project_id <> s.project_id
              OR length(CAST(s.id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(s.harness_id AS BLOB)) NOT BETWEEN 1 AND 512
              OR (s.harness_version IS NOT NULL AND length(CAST(s.harness_version AS BLOB)) NOT BETWEEN 1 AND 512)
              OR (s.model_id IS NOT NULL AND length(CAST(s.model_id AS BLOB)) NOT BETWEEN 1 AND 512)
              OR (s.external_session_id IS NOT NULL AND length(CAST(s.external_session_id AS BLOB)) NOT BETWEEN 1 AND 512)
              OR s.last_committed_sequence <> COALESCE((
                   SELECT MAX(e.sequence) FROM session_events e WHERE e.session_id = s.id
                 ), 0)
              OR EXISTS(
                   SELECT 1 FROM session_events e
                   WHERE e.session_id = s.id AND e.sequence <= 0
                 )
              OR (SELECT COUNT(*) FROM session_events e WHERE e.session_id = s.id)
                 <> s.last_committed_sequence
           LIMIT 1
         )",
        [],
        |row| row.get(0),
    )?;
    if invalid {
        return Err(AppError::InvalidRequest(
            "legacy session ownership or sequence metadata is corrupt".into(),
        ));
    }
    Ok(())
}

pub(super) fn verify_file_envelope(live: &Path) -> AppResult<()> {
    let main = fs::metadata(live)?.len();
    if main > LIVE_SQLITE_BYTES {
        return limit("live SQLite image exceeds 1 GiB");
    }
    let wal = sidecar_len(live, "-wal")?;
    if wal > WAL_HARD_BYTES {
        return limit("live WAL exceeds the 32 MiB hard stop");
    }
    let shm = sidecar_len(live, "-shm")?;
    if shm.saturating_add(MANIFEST_BYTES.saturating_mul(4)) > SIDE_FILES_BYTES {
        return limit("SQLite SHM and migration manifests exceed 1 MiB");
    }
    Ok(())
}

fn sidecar_len(live: &Path, suffix: &str) -> AppResult<u64> {
    let path = live.with_file_name(format!(
        "{}{}",
        live.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                AppError::InvalidRequest("Sage database filename is not UTF-8".into())
            })?,
        suffix
    ));
    Ok(match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    })
}

fn table_exists(connection: &Connection, name: &str) -> AppResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
        params![name],
        |row| row.get(0),
    )?)
}

fn limit<T>(message: &str) -> AppResult<T> {
    Err(AppError::ResourceLimit(message.into()))
}
