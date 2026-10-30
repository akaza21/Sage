use rusqlite::Connection;

use crate::{error::AppResult, project_migration};

use super::super::{INITIAL, V2, V3, V6, V7, V8, V9, V10, V11, V12, V13, V14, V15, V16, legacy};
use super::{V10_LEGACY_SESSION_EVENTS, schema_fingerprint};

pub(super) fn expected_schema_fingerprint(version: u32) -> AppResult<String> {
    schema_fingerprint(&expected_schema_connection(version)?)
}

pub(super) fn expected_legacy_v10_session_events_fingerprint() -> AppResult<String> {
    let expected = expected_schema_connection(10)?;
    expected.execute_batch(V10_LEGACY_SESSION_EVENTS)?;
    schema_fingerprint(&expected)
}

fn expected_schema_connection(version: u32) -> AppResult<Connection> {
    let mut expected = Connection::open_in_memory()?;
    expected.execute_batch(INITIAL)?;
    if version >= 2 {
        expected.execute_batch(V2)?;
    }
    if version >= 3 {
        expected.execute_batch(V3)?;
    }
    if version >= 4 {
        project_migration::migrate(&mut expected)?;
    }
    if version >= 5 {
        let transaction = expected.transaction()?;
        legacy::convert(&transaction, 0)?;
        transaction.commit()?;
    }
    if version >= 6 {
        expected.execute_batch(V6)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (6, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 6)?;
    }
    if version >= 7 {
        expected.pragma_update(None, "foreign_keys", "OFF")?;
        expected.execute_batch(V7)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (7, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 7)?;
    }
    if version >= 8 {
        expected.pragma_update(None, "foreign_keys", "OFF")?;
        expected.execute_batch(V8)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (8, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 8)?;
    }
    if version >= 9 {
        expected.pragma_update(None, "foreign_keys", "OFF")?;
        expected.execute_batch(V9)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (9, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 9)?;
    }
    if version >= 10 {
        expected.pragma_update(None, "foreign_keys", "OFF")?;
        expected.execute_batch(V10)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (10, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 10)?;
    }
    if version >= 11 {
        expected.pragma_update(None, "foreign_keys", "OFF")?;
        expected.execute_batch(V11)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (11, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 11)?;
    }
    if version >= 12 {
        expected.pragma_update(None, "foreign_keys", "OFF")?;
        expected.execute_batch(V12)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (12, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 12)?;
    }
    if version >= 13 {
        expected.execute_batch(V13)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (13, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 13)?;
    }
    if version >= 14 {
        expected.execute_batch(V14)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (14, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 14)?;
    }
    if version >= 15 {
        expected.execute_batch(V15)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (15, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 15)?;
    }
    if version >= 16 {
        expected.execute_batch(V16)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (16, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 16)?;
    }
    if version >= 17 {
        super::super::schema_upgrade::apply_v17_steps(&expected)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (17, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 17)?;
    }
    if version >= 18 {
        super::super::schema_upgrade::apply_v18_steps(&expected)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (18, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 18)?;
    }
    if version >= 19 {
        expected.execute_batch(super::super::workspace_changes::V19)?;
        expected.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (19, 0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 19)?;
    }
    if version >= 20 {
        expected.execute_batch(super::super::context_handoff::V20)?;
        expected.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(20,0)",
            [],
        )?;
        expected.pragma_update(None, "user_version", 20)?;
    }
    Ok(expected)
}
