use super::*;
use crate::acp::store::{
    StartSessionCommand, commands::sign_in_evidence::RefusalCommand, start_session,
};
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use sage_acp_protocol::{contract::*, intent::BindingId};
use sage_db::{
    database::{Database, DatabaseOptions},
    physical_storage::StorageLimits,
};

#[test]
fn storage_gate_blocks_refusal_evidence_without_hiding_the_sign_in_error() {
    let directory =
        std::env::temp_dir().join(format!("sage-refusal-gate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let database = crate::database::open_database(&path).unwrap();
    let project = database.create_project_from_root(&directory).unwrap();
    let chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let session = start_session(
        &mut database.write_connection().unwrap(),
        StartSessionCommand {
            project_id: SageProjectId::parse(&project.id).unwrap(),
            chat_id: SageChatId::parse(&chat).unwrap(),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("start").unwrap(),
            binding_id: BindingId::new("binding", "fixture").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: None,
            now_millis: 10,
        },
    )
    .unwrap()
    .session_id;
    database
        .connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(database);
    let (connection, anchor) = crate::acp::migration::open_or_migrate(&path)
        .unwrap()
        .into_parts();
    let database = Database::open(
        &path,
        connection,
        anchor,
        DatabaseOptions {
            reader_busy_timeout: std::time::Duration::from_millis(10),
            storage_limits: StorageLimits::fixture(16 * 1024 * 1024, 16 * 1024, 32 * 1024),
        },
    )
    .unwrap();
    let state = crate::state::AppState::new(database);
    let database = &state.database;
    database
        .connection()
        .busy_timeout(std::time::Duration::from_millis(10))
        .unwrap();
    database
        .connection()
        .execute_batch(
            "CREATE TABLE wal_gate_fixture(payload BLOB); PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .unwrap();
    let reader = database.pinned_reader().unwrap().unwrap();
    for _ in 0..8 {
        let Ok(connection) = database.write_connection() else {
            break;
        };
        connection
            .execute("INSERT INTO wal_gate_fixture VALUES(zeroblob(8192))", [])
            .unwrap();
    }
    assert!(database.storage_recovery_required());
    assert!(matches!(
        database.write_connection(),
        Err(sage_core::error::AppError::ResourceLimit(_))
    ));
    let wal = path.with_file_name("sage.db-wal");
    let before = std::fs::metadata(&wal).unwrap().len();
    let refuse = || {
        persisted_sign_in_error(
            &state,
            RefusalCommand {
                project_id: &project.id,
                chat_id: &chat,
                client_intent_id: "config",
                session_id: Some(session.as_str()),
            },
            session_error(crate::acp::service::CodexSessionError::SignInRequired),
        )
    };
    assert!(matches!(
        refuse(),
        AcpIpcError::SignInRequired {
            terminal_at: None,
            ..
        }
    ));
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), before);
    let count: i64 = state
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='commandSignInRefusal'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(reader);
    assert!(matches!(
        refuse(),
        AcpIpcError::SignInRequired {
            terminal_at: Some(_),
            ..
        }
    ));
    assert!(!state.database.storage_recovery_required());
    drop(state);
    std::fs::remove_dir_all(directory).unwrap();
}
