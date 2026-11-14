use std::sync::OnceLock;

use rusqlite::Connection;
use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;

use crate::project_migration;

use super::super::{INITIAL, V2, V3, V6, V7, V8, V9, V10, V11, V12, V13, V14, V15, V16, legacy};
use super::{MAX_SCHEMA_VERSION, V10_LEGACY_SESSION_EVENTS, schema_fingerprint};

// An expected fingerprint depends only on the SQL built into this binary, so
// each one is replayed once per process. Every migration step classifies its
// source, and replaying the whole chain each time made opening a fresh
// database quadratic in the schema version.
static EXPECTED: [OnceLock<String>; MAX_SCHEMA_VERSION + 1] =
    [const { OnceLock::new() }; MAX_SCHEMA_VERSION + 1];
static EXPECTED_LEGACY_V10_SESSION_EVENTS: OnceLock<String> = OnceLock::new();

pub(super) fn expected_schema_fingerprint(version: u32) -> AppResult<String> {
    let replay = || schema_fingerprint(&expected_schema_connection(version)?);
    match usize::try_from(version)
        .ok()
        .and_then(|index| EXPECTED.get(index))
    {
        Some(slot) => memoized(slot, replay),
        None => replay(),
    }
}

pub(super) fn expected_legacy_v10_session_events_fingerprint() -> AppResult<String> {
    memoized(&EXPECTED_LEGACY_V10_SESSION_EVENTS, || {
        let expected = expected_schema_connection(10)?;
        expected.execute_batch(V10_LEGACY_SESSION_EVENTS).db()?;
        schema_fingerprint(&expected)
    })
}

/// A failed replay is returned and not cached, so the next call retries it.
fn memoized(
    slot: &OnceLock<String>,
    replay: impl FnOnce() -> AppResult<String>,
) -> AppResult<String> {
    if let Some(fingerprint) = slot.get() {
        return Ok(fingerprint.clone());
    }
    let fingerprint = replay()?;
    Ok(slot.get_or_init(|| fingerprint).clone())
}

fn expected_schema_connection(version: u32) -> AppResult<Connection> {
    let mut expected = Connection::open_in_memory().db()?;
    expected.execute_batch(INITIAL).db()?;
    if version >= 2 {
        expected.execute_batch(V2).db()?;
    }
    if version >= 3 {
        expected.execute_batch(V3).db()?;
    }
    if version >= 4 {
        project_migration::migrate(&mut expected)?;
    }
    if version >= 5 {
        let transaction = expected.transaction().db()?;
        legacy::convert(&transaction, 0)?;
        transaction.commit().db()?;
    }
    if version >= 6 {
        expected.execute_batch(V6).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (6, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 6).db()?;
    }
    if version >= 7 {
        expected.pragma_update(None, "foreign_keys", "OFF").db()?;
        expected.execute_batch(V7).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (7, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 7).db()?;
    }
    if version >= 8 {
        expected.pragma_update(None, "foreign_keys", "OFF").db()?;
        expected.execute_batch(V8).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (8, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 8).db()?;
    }
    if version >= 9 {
        expected.pragma_update(None, "foreign_keys", "OFF").db()?;
        expected.execute_batch(V9).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (9, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 9).db()?;
    }
    if version >= 10 {
        expected.pragma_update(None, "foreign_keys", "OFF").db()?;
        expected.execute_batch(V10).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (10, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 10).db()?;
    }
    if version >= 11 {
        expected.pragma_update(None, "foreign_keys", "OFF").db()?;
        expected.execute_batch(V11).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (11, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 11).db()?;
    }
    if version >= 12 {
        expected.pragma_update(None, "foreign_keys", "OFF").db()?;
        expected.execute_batch(V12).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (12, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 12).db()?;
    }
    if version >= 13 {
        expected.execute_batch(V13).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (13, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 13).db()?;
    }
    if version >= 14 {
        expected.execute_batch(V14).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (14, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 14).db()?;
    }
    if version >= 15 {
        expected.execute_batch(V15).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (15, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 15).db()?;
    }
    if version >= 16 {
        expected.execute_batch(V16).db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (16, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 16).db()?;
    }
    if version >= 17 {
        super::super::schema_upgrade::apply_v17_steps(&expected)?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (17, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 17).db()?;
    }
    if version >= 18 {
        super::super::schema_upgrade::apply_v18_steps(&expected)?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (18, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 18).db()?;
    }
    if version >= 19 {
        expected
            .execute_batch(super::super::workspace_changes::V19)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (19, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 19).db()?;
    }
    if version >= 20 {
        expected
            .execute_batch(super::super::title_provenance::V20)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (20, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 20).db()?;
    }
    if version >= 21 {
        expected
            .execute_batch(super::super::session_scope::V21)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (21, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 21).db()?;
    }
    if version >= 22 {
        expected
            .execute_batch(super::super::notifications::V22)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (22, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 22).db()?;
    }
    if version >= 23 {
        expected
            .execute_batch(super::super::config_choices::V23)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (23, 0)",
                [],
            )
            .db()?;
        expected.pragma_update(None, "user_version", 23).db()?;
    }
    let resume_link = super::super::session_resume_link::VERSION;
    if version >= resume_link {
        expected
            .execute_batch(super::super::session_resume_link::SQL)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [resume_link],
            )
            .db()?;
        expected
            .pragma_update(None, "user_version", resume_link)
            .db()?;
    }
    let title_source = super::super::title_source::VERSION;
    if version >= title_source {
        expected
            .execute_batch(super::super::title_source::SQL)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [title_source],
            )
            .db()?;
        expected
            .pragma_update(None, "user_version", title_source)
            .db()?;
    }
    let turn_index = super::super::session_facts_turn_index::VERSION;
    if version >= turn_index {
        expected
            .execute_batch(super::super::session_facts_turn_index::SQL)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [turn_index],
            )
            .db()?;
        expected
            .pragma_update(None, "user_version", turn_index)
            .db()?;
    }
    let terminal_index = super::super::operation_terminal_index::VERSION;
    if version >= terminal_index {
        expected
            .execute_batch(super::super::operation_terminal_index::SQL)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [terminal_index],
            )
            .db()?;
        expected
            .pragma_update(None, "user_version", terminal_index)
            .db()?;
    }
    let undo_indexes = super::super::turn_undo_intent_indexes::VERSION;
    if version >= undo_indexes {
        expected
            .execute_batch(super::super::turn_undo_intent_indexes::SQL)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [undo_indexes],
            )
            .db()?;
        expected
            .pragma_update(None, "user_version", undo_indexes)
            .db()?;
    }
    let imported = super::super::imported_history::VERSION;
    if version >= imported {
        expected
            .execute_batch(super::super::imported_history::SQL)
            .db()?;
        expected
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, 0)",
                [imported],
            )
            .db()?;
        expected
            .pragma_update(None, "user_version", imported)
            .db()?;
    }
    Ok(expected)
}
