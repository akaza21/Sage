use super::*;

const REQUEST_ID: &str = "permission-v8";

fn populated_v8() -> (TestDir, fingerprint::DatabaseFingerprint) {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).expect("v8 database");
    connection.execute_batch(INITIAL).expect("v1");
    connection.execute_batch(V2).expect("v2");
    connection.execute_batch(V3).expect("v3");
    project_migration::migrate(&mut connection).expect("v4");
    apply_v5(&mut connection).expect("v5");
    apply_v6(&mut connection).expect("v6");
    apply_v7(&mut connection).expect("v7");
    apply_v8(&mut connection).expect("v8");
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES ('project-v8','Project v8',1,1,1)",
            [],
        )
        .expect("project");
    connection
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
             VALUES ('chat-v8','project-v8',0,'Chat v8',1,1)",
            [],
        )
        .expect("chat");
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,
               capabilities_json,state,residency,legacy,revision,
               projection_revision,created_at,updated_at,terminal_at
             ) VALUES ('session-v8','project-v8','chat-v8',0,'binding',?1,?2,
                       'starting','hot',0,0,0,1,1,NULL)",
            params!["a".repeat(64), PENDING_CAPABILITIES],
        )
        .expect("session");
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,
               started_at,terminal_at,exit_kind
             ) VALUES ('project-v8','chat-v8','session-v8',1,'starting',?1,1,NULL,NULL)",
            ["b".repeat(64)],
        )
        .expect("epoch");
    connection
        .execute(
            "INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
               state,delivery_state,revision,created_at,updated_at,terminal_at
             ) VALUES ('turn-v8','project-v8','chat-v8','session-v8',1,0,'intent-v8',
                       'running','completeBytes',1,1,1,NULL)",
            [],
        )
        .expect("turn");
    connection
        .execute(
            "INSERT INTO server_requests(
               id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
               external_request_id,kind,payload_digest_version,payload_digest,state,
               normalized_payload_json,revision,created_at,updated_at,resolved_at
             ) VALUES (?1,'project-v8','chat-v8','session-v8','turn-v8',1,?2,
                       'external-v8','permissionRequest',1,?3,'asked','{}',3,1,2,NULL)",
            params![REQUEST_ID, "a".repeat(64), "c".repeat(64)],
        )
        .expect("permission request");
    let source = database_fingerprint(&connection).expect("source fingerprint");
    assert_eq!(source.schema, SchemaClass::CurrentV8);
    connection.close().expect("close v8");
    publish_retained_backup(&directory);
    (directory, source)
}

fn publish_retained_backup(directory: &TestDir) {
    let paths = MigrationPaths::new(&directory.database()).expect("paths");
    let source = open_read_only(&paths.live).expect("source");
    let source_set = backup::source_set(&paths, &source).expect("source set");
    drop(source);
    fs::copy(&paths.live, &paths.backup_final).expect("backup copy");
    let backup = open_read_only(&paths.backup_final).expect("backup");
    let backup_database = database_fingerprint(&backup).expect("backup database");
    drop(backup);
    let manifest = manifest::BackupManifest::new(
        source_set,
        file_fingerprint(&paths.backup_final).expect("backup file"),
        backup_database,
        1,
    );
    manifest::write_stage(&paths.manifest_final, &manifest).expect("backup manifest");
    backup::verify_retained_pair(&paths).expect("retained pair");
}

fn assert_populated_rows(connection: &Connection) {
    for table in [
        "projects",
        "chats",
        "agent_sessions",
        "session_runtime_epochs",
        "turns",
        "server_requests",
    ] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("preserved row count"),
            1,
            "unexpected row count for {table}"
        );
    }
    let row: (
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        i64,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT project_id,chat_id,session_id,turn_id,runtime_epoch,
                    external_request_id,state,revision,resolved_at
             FROM server_requests WHERE id=?1",
            [REQUEST_ID],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .expect("preserved request");
    assert_eq!(
        row,
        (
            "project-v8".into(),
            "chat-v8".into(),
            "session-v8".into(),
            "turn-v8".into(),
            1,
            "external-v8".into(),
            "asked".into(),
            3,
            None,
        )
    );
}

fn assert_v13_structure(connection: &Connection) {
    assert_eq!(
        fingerprint::classify_connection(connection).expect("classification"),
        SchemaClass::CurrentV13
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
                .get::<_, i64>(0))
            .expect("migration count"),
        13
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='chat_queued_prompts'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("queued prompt table"),
        1
    );
    assert!(
        connection
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
            .optional()
            .expect("foreign keys")
            .is_none()
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type='index' AND name='idx_server_requests_owner'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("owner index"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type='table' AND name='acp_file_change_snapshots'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("snapshot table"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type='index' AND name='idx_file_change_snapshots_turn'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("snapshot turn index"),
        1
    );
    for trigger in [
        "deny_server_request_delete",
        "deny_server_request_identity_mutation",
        "deny_server_request_invalid_transition",
        "deny_terminal_server_request_mutation",
        "deny_file_change_snapshot_mutation",
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name=?1",
                    [trigger],
                    |row| row.get::<_, i64>(0),
                )
                .expect("trigger"),
            1,
            "missing exact trigger {trigger}"
        );
    }
}

#[test]
fn populated_v8_upgrades_exactly_and_reopens_idempotently_with_retained_source() {
    let (directory, source) = populated_v8();
    let fresh = open_or_migrate(Path::new(":memory:")).expect("fresh v13");
    let expected_schema = database_fingerprint(&fresh)
        .expect("fresh fingerprint")
        .schema_sha256;

    let migrated = open_or_migrate(&directory.database()).expect("v13 migration");
    assert_v13_structure(&migrated);
    assert_populated_rows(&migrated);
    let migrated_fingerprint = database_fingerprint(&migrated).expect("migrated fingerprint");
    assert_eq!(migrated_fingerprint.schema_sha256, expected_schema);
    drop(migrated);

    let paths = MigrationPaths::new(&directory.database()).expect("paths");
    backup::verify_retained_pair(&paths).expect("retained backup");
    let backup = open_read_only(&paths.backup_final).expect("backup");
    assert_eq!(
        database_fingerprint(&backup).expect("backup fingerprint"),
        source
    );
    drop(backup);

    let reopened = open_or_migrate(&directory.database()).expect("idempotent reopen");
    assert_v13_structure(&reopened);
    assert_populated_rows(&reopened);
    assert_eq!(
        database_fingerprint(&reopened).expect("reopen fingerprint"),
        migrated_fingerprint
    );
}

#[test]
fn populated_v8_injected_v9_failure_rolls_back_and_retry_reuses_retained_backup() {
    let (directory, source) = populated_v8();
    test_failpoint::arm("v8-to-v9-after-schema");

    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!test_failpoint::is_armed(), "v9 failpoint was not reached");

    let rolled_back = open_read_only(&directory.database()).expect("rolled-back source");
    assert_eq!(
        database_fingerprint(&rolled_back).expect("source fingerprint"),
        source
    );
    assert_populated_rows(&rolled_back);
    drop(rolled_back);
    let paths = MigrationPaths::new(&directory.database()).expect("paths");
    backup::verify_retained_pair(&paths).expect("retained backup");
    let backup = open_read_only(&paths.backup_final).expect("backup");
    assert_eq!(
        database_fingerprint(&backup).expect("backup fingerprint"),
        source
    );
    drop(backup);

    let retried = open_or_migrate(&directory.database()).expect("retry");
    assert_v13_structure(&retried);
    assert_populated_rows(&retried);
    drop(retried);
    backup::verify_retained_pair(&paths).expect("retained backup after retry");
}
