//! Portable coverage for operations which do not require Windows' identity-bound deletion.

use std::{fs, path::PathBuf, time::Duration};

use rusqlite::{Connection, backup::Backup, params};
use uuid::Uuid;

use crate::{persistence::Database, project_migration};

use super::{
    INITIAL, MigrationPaths, V2, V3,
    backup::load_retained_pair,
    fingerprint::{SchemaClass, database_fingerprint, file_fingerprint},
    manifest::{BackupManifest, BackupSource, ReplacementManifest, SourceSet, write_stage},
    platform::{remove_synced, rename_synced},
    publish::recover_or_publish,
    schema_upgrade::{apply_through_v13, apply_v5, apply_v6},
    test_failpoint,
};

const PENDING_CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"pending\",\"newSession\":false,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":false,\"prompt\":false,\"cancel\":false,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":0,\"configCount\":0}";

struct TestDir(PathBuf);

struct OfflineWalFixture {
    source: SourceSet,
    wal_bytes: Vec<u8>,
    shm_bytes: Vec<u8>,
}

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-portable-migration-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("sage.db")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn portable_apply_through_v13_dispatches_exact_v1_v2_v3_v5_and_v6_sources() {
    for version in [1_i64, 2, 3, 5, 6] {
        let directory = TestDir::new();
        let mut connection = Connection::open(directory.database()).unwrap();
        connection.execute_batch(INITIAL).unwrap();
        if version >= 2 {
            connection.execute_batch(V2).unwrap();
        }
        if version >= 3 {
            connection.execute_batch(V3).unwrap();
        }
        if version >= 4 {
            project_migration::migrate(&mut connection).unwrap();
        }
        if version >= 5 {
            apply_v5(&mut connection).unwrap();
        }
        if version >= 6 {
            apply_v6(&mut connection).unwrap();
            seed_epoch_child(&connection);
        }

        apply_through_v13(&mut connection).unwrap();

        assert_eq!(
            super::fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV13,
            "source version {version}"
        );
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(migration_count, 13);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_xinfo('resource_trash_intents')",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            13
        );
        if version >= 6 {
            assert_eq!(
                connection
                    .query_row("SELECT name FROM projects WHERE id='project'", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .unwrap(),
                "Project"
            );
            assert_eq!(
                connection
                    .query_row("SELECT COUNT(*) FROM turns WHERE id='turn'", [], |row| row
                        .get::<_, i64>(
                        0
                    ))
                    .unwrap(),
                1
            );
            assert_eq!(
                connection
                    .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                0
            );
        }
    }
}

#[test]
fn portable_v5_dispatch_rolls_back_v6_v7_failpoint_then_retries() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    seed_current_v5(&mut connection);
    test_failpoint::arm("v5-to-v7-after-v6");
    assert!(apply_through_v13(&mut connection).is_err());
    assert_eq!(
        super::fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV5
    );
    apply_through_v13(&mut connection).unwrap();
    assert_eq!(
        super::fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV13
    );
}

#[test]
fn portable_v4_transform_then_database_runtime_preserves_simulator_restart_and_rollback() {
    let directory = TestDir::new();
    create_v4(&directory.database(), true);
    let mut upgrade = Connection::open(directory.database()).unwrap();
    apply_through_v13(&mut upgrade).unwrap();
    upgrade.close().unwrap();

    let database = Database::open(&directory.database()).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    assert_eq!(initial.session_id, None);
    assert_eq!(initial.messages.len(), 1);
    assert_eq!(initial.messages[0].id, "event-1");
    let before = simulator_state(&database);
    let rollback = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        database.append_simulator_turn_with_hook("chat-a", "continue", "ack", || panic!("injected"))
    }));
    assert!(rollback.is_err());
    assert_eq!(simulator_state(&database), before);
    drop(database);
    let database = Database::open(&directory.database()).unwrap();
    assert_eq!(simulator_state(&database), before);
    let after_rollback = database.bootstrap_snapshot().unwrap();
    assert_eq!(after_rollback.session_id, None);
    assert_eq!(after_rollback.messages[0].id, "event-1");
    let first = database
        .append_simulator_turn("chat-a", "continue", "ack")
        .unwrap();
    let session = first.session_id.clone();
    assert_ne!(session, "session-a");
    assert_eq!(first.messages[0].id, "event-1");
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT state,residency,legacy,ordinal FROM agent_sessions WHERE id='session-a'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?
                )),
            )
            .unwrap(),
        ("closed".into(), "cold".into(), 1, 0)
    );
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT state,residency,legacy,ordinal FROM agent_sessions WHERE id=?1",
                [&session],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?
                )),
            )
            .unwrap(),
        ("ready".into(), "cold".into(), 0, 1)
    );
    let second = database
        .append_simulator_turn("chat-a", "again", "ack")
        .unwrap();
    assert_eq!(second.session_id, session);
    drop(database);

    let restarted = Database::open(&directory.database()).unwrap();
    let snapshot = restarted.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.session_id.as_deref(), Some(session.as_str()));
    assert_eq!(snapshot.messages.len(), 5);
    assert_eq!(snapshot.messages[0].id, "event-1");
}

#[test]
fn portable_v4_transform_then_database_runtime_creates_first_empty_chat_prompt() {
    let directory = TestDir::new();
    create_v4(&directory.database(), false);
    let mut upgrade = Connection::open(directory.database()).unwrap();
    apply_through_v13(&mut upgrade).unwrap();
    upgrade.close().unwrap();

    let database = Database::open(&directory.database()).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    assert_eq!(initial.session_id, None);
    assert!(initial.messages.is_empty());
    let turn = database
        .append_simulator_turn("chat-a", "first prompt", "ack")
        .unwrap();
    assert_eq!(turn.messages.len(), 2);
    assert!(
        turn.messages
            .iter()
            .all(|message| message.session_id == turn.session_id)
    );
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT ordinal,state,residency,legacy FROM agent_sessions WHERE id=?1",
                [&turn.session_id],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?
                )),
            )
            .unwrap(),
        (0, "ready".into(), "cold".into(), 0)
    );
    drop(database);
    let restarted = Database::open(&directory.database()).unwrap();
    let restored = restarted.bootstrap_snapshot().unwrap();
    assert_eq!(restored.session_id, Some(turn.session_id));
    assert_eq!(restored.messages.len(), 2);
}

#[test]
fn portable_publish_recovers_valid_prefixes_and_rejects_stale_stage_identity() {
    let directory = TestDir::new();
    create_v4(&directory.database(), true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    prepare_populated_replacement(&paths);

    assert!(recover_or_publish(&paths).unwrap());
    assert_eq!(
        super::fingerprint::classify_path(&paths.live).unwrap(),
        SchemaClass::CurrentV13
    );
    let published = Connection::open(&paths.live).unwrap();
    assert_eq!(
        published
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "Project"
    );
    drop(published);
    assert!(!paths.replacement_manifest.exists());
    assert!(!paths.retired_live.exists());
    assert!(!recover_or_publish(&paths).unwrap());

    let bad = TestDir::new();
    create_v4(&bad.database(), false);
    let bad_paths = MigrationPaths::new(&bad.database()).unwrap();
    let bad_source = source_set(&bad_paths);
    let mut stage = Connection::open(&bad_paths.replacement_stage).unwrap();
    super::schema_upgrade::build_fresh_v9(&mut stage).unwrap();
    stage.close().unwrap();
    let stage_connection = Connection::open(&bad_paths.replacement_stage).unwrap();
    let staged_database = database_fingerprint(&stage_connection).unwrap();
    drop(stage_connection);
    let stale = ReplacementManifest::new(
        BackupSource {
            database: bad_source.database.clone(),
            main: bad_source.main.clone(),
            wal: bad_source.wal.clone(),
        },
        bad_source,
        file_fingerprint(&bad_paths.live).unwrap(),
        file_fingerprint(&bad_paths.replacement_stage).unwrap(),
        staged_database,
    );
    write_stage(&bad_paths.replacement_manifest, &stale).unwrap();
    fs::write(&bad_paths.replacement_stage, b"stale stage").unwrap();
    assert!(recover_or_publish(&bad_paths).is_err());
    assert_eq!(
        fs::read(&bad_paths.replacement_stage).unwrap(),
        b"stale stage"
    );
}

#[test]
fn portable_publish_recovers_retirement_restore_and_cleanup_prefixes() {
    for prefix in ["none", "wal", "shm", "main"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let fixture = prepare_offline_wal_fixture(&paths);
        let manifest = prepare_populated_wal_replacement(&paths, &fixture);
        retire_prefix(&paths, &manifest, prefix);

        assert!(
            recover_or_publish(&paths).unwrap(),
            "retirement prefix {prefix}"
        );
        assert_published_project(&paths);
        assert_wal_update(&paths);
        assert!(!paths.replacement_stage.exists());
        assert!(!paths.retired_live.exists());
        assert!(!paths.retired_wal.exists());
        assert!(!paths.retired_shm.exists());
        assert!(!paths.replacement_manifest.exists());
        assert!(!recover_or_publish(&paths).unwrap());
    }
}

#[test]
fn portable_publish_restores_and_cleans_wal_shm_prefixes_in_order() {
    for prefix in ["none", "shm", "wal"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let fixture = prepare_offline_wal_fixture(&paths);
        let manifest = prepare_populated_wal_replacement(&paths, &fixture);
        retire_prefix(&paths, &manifest, "main");
        remove_synced(&paths.replacement_stage).unwrap();
        restore_prefix(&paths, &manifest, prefix);

        assert!(
            !recover_or_publish(&paths).unwrap(),
            "restore prefix {prefix}"
        );
        assert_eq!(
            super::fingerprint::classify_path(&paths.live).unwrap(),
            SchemaClass::LegacyV4
        );
        assert_legacy_project(&paths);
        assert_wal_update(&paths);
        assert!(!paths.retired_live.exists());
        assert!(!paths.retired_wal.exists());
        assert!(!paths.retired_shm.exists());
        assert!(!paths.replacement_manifest.exists());
    }

    for prefix in ["none", "shm", "wal", "main"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let fixture = prepare_offline_wal_fixture(&paths);
        let manifest = prepare_populated_wal_replacement(&paths, &fixture);
        retire_prefix(&paths, &manifest, "main");
        rename_synced(&paths.replacement_stage, &paths.live).unwrap();
        cleanup_prefix(&paths, prefix);

        assert!(
            recover_or_publish(&paths).unwrap(),
            "cleanup prefix {prefix}"
        );
        assert_published_project(&paths);
        assert_wal_update(&paths);
        assert!(!paths.retired_live.exists());
        assert!(!paths.retired_wal.exists());
        assert!(!paths.retired_shm.exists());
        assert!(!paths.replacement_manifest.exists());
        assert!(!recover_or_publish(&paths).unwrap());
    }
}

#[test]
fn portable_retained_backup_survives_build_failure_then_retry_publishes() {
    let directory = TestDir::new();
    create_v4(&directory.database(), true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let fixture = prepare_offline_wal_fixture(&paths);
    let retained = install_retained_pair(&paths, &fixture.source);
    fixture.materialize(&paths);
    fixture.assert_materialized(&paths);
    let source_before = artifact_bytes(&paths);
    let backup_before = fs::read(&paths.backup_final).unwrap();
    let backup_manifest_before = fs::read(&paths.manifest_final).unwrap();

    fs::copy(&paths.backup_final, &paths.replacement_stage).unwrap();
    let mut replacement = Connection::open(&paths.replacement_stage).unwrap();
    test_failpoint::arm("v8-to-v9-after-schema");
    assert!(apply_through_v13(&mut replacement).is_err());
    replacement.close().unwrap();

    assert_eq!(artifact_bytes(&paths), source_before);
    assert_eq!(fs::read(&paths.backup_final).unwrap(), backup_before);
    assert_eq!(
        fs::read(&paths.manifest_final).unwrap(),
        backup_manifest_before
    );
    load_retained_pair(&paths).unwrap();
    assert!(!paths.replacement_manifest.exists());
    assert!(!recover_or_publish(&paths).unwrap());

    let mut replacement = Connection::open(&paths.replacement_stage).unwrap();
    apply_through_v13(&mut replacement).unwrap();
    let replacement_database = database_fingerprint(&replacement).unwrap();
    replacement.close().unwrap();
    let manifest = ReplacementManifest::new(
        BackupSource {
            database: fixture.source.database.clone(),
            main: fixture.source.main.clone(),
            wal: fixture.source.wal.clone(),
        },
        fixture.source.clone(),
        retained.backup_file,
        file_fingerprint(&paths.replacement_stage).unwrap(),
        replacement_database,
    );
    write_stage(&paths.replacement_manifest, &manifest).unwrap();
    assert!(recover_or_publish(&paths).unwrap());
    load_retained_pair(&paths).unwrap();
    assert_published_project(&paths);
    assert_wal_update(&paths);
}

#[test]
fn portable_publish_rejects_invalid_retirement_order_without_mutating_artifacts() {
    let directory = TestDir::new();
    create_v4(&directory.database(), true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let _manifest = prepare_populated_replacement(&paths);
    fs::copy(&paths.live, &paths.retired_live).unwrap();
    let live_before = fs::read(&paths.live).unwrap();
    let retired_before = fs::read(&paths.retired_live).unwrap();

    assert!(recover_or_publish(&paths).is_err());
    assert_eq!(fs::read(&paths.live).unwrap(), live_before);
    assert_eq!(fs::read(&paths.retired_live).unwrap(), retired_before);
    assert!(paths.replacement_stage.exists());
    assert!(paths.replacement_manifest.exists());
}

fn seed_current_v5(connection: &mut Connection) {
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(V2).unwrap();
    connection.execute_batch(V3).unwrap();
    project_migration::migrate(connection).unwrap();
    apply_v5(connection).unwrap();
}

fn seed_epoch_child(connection: &Connection) {
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('project','Project',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES ('chat','project',0,'Chat',1,1)", []).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at,terminal_at) VALUES ('session','project','chat',0,'binding',?1,?2,'starting','hot',0,0,0,1,1,NULL)", params!["a".repeat(64), PENDING_CAPABILITIES]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at,terminal_at,exit_kind) VALUES ('project','chat','session',1,'starting',?1,1,NULL,NULL)", ["b".repeat(64)]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at) VALUES ('turn','project','chat','session',1,0,'intent','queued','notWritten',0,1,1,NULL)", []).unwrap();
}

fn create_v4(path: &PathBuf, event: bool) {
    let mut connection = Connection::open(path).unwrap();
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(V2).unwrap();
    connection.execute_batch(V3).unwrap();
    project_migration::migrate(&mut connection).unwrap();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('project-a','Project',1,1,1)", []).unwrap();
    let root = path.parent().unwrap().join("root");
    fs::create_dir_all(&root).unwrap();
    let canonical = root.canonicalize().unwrap();
    connection.execute("INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES ('root-a','project-a',?1,?2,0)", params![canonical.to_string_lossy().into_owned(), crate::project_registry::canonical_comparison_key(&canonical)]).unwrap();
    connection.execute("INSERT INTO workbench_tabs(id,project_id,kind,title,position,pinned,dirty,status,metadata_json,created_at,updated_at) VALUES ('chat-a','project-a','chat','Chat',0,0,0,NULL,'{}',1,1)", []).unwrap();
    if event {
        connection.execute("INSERT INTO agent_sessions(id,project_id,chat_tab_id,harness_id,status,last_committed_sequence,created_at,updated_at) VALUES ('session-a','project-a','chat-a','simulator','complete',1,1,2)", []).unwrap();
        connection.execute("INSERT INTO session_events(id,session_id,sequence,kind,payload_json,created_at) VALUES ('event-1','session-a',1,'message','{\"role\":\"assistant\",\"text\":\"hello\"}',2)", []).unwrap();
    }
    connection.close().unwrap();
}

fn source_set(paths: &MigrationPaths) -> SourceSet {
    let connection = Connection::open(&paths.live).unwrap();
    let database = database_fingerprint(&connection).unwrap();
    drop(connection);
    SourceSet {
        database,
        main: file_fingerprint(&paths.live).unwrap(),
        wal: file_fingerprint(&paths.live_wal).unwrap(),
        shm: file_fingerprint(&paths.live_shm).unwrap(),
    }
}

fn prepare_populated_replacement(paths: &MigrationPaths) -> ReplacementManifest {
    let source = source_set(paths);
    let source_before = BackupSource {
        database: source.database.clone(),
        main: source.main.clone(),
        wal: source.wal.clone(),
    };
    fs::copy(&paths.live, &paths.replacement_stage).unwrap();
    let mut replacement = Connection::open(&paths.replacement_stage).unwrap();
    apply_through_v13(&mut replacement).unwrap();
    replacement.close().unwrap();
    let replacement_file = file_fingerprint(&paths.replacement_stage).unwrap();
    let replacement_connection = Connection::open(&paths.replacement_stage).unwrap();
    let replacement_database = database_fingerprint(&replacement_connection).unwrap();
    drop(replacement_connection);
    let manifest = ReplacementManifest::new(
        source_before,
        source,
        file_fingerprint(&paths.live).unwrap(),
        replacement_file,
        replacement_database,
    );
    write_stage(&paths.replacement_manifest, &manifest).unwrap();
    manifest
}

fn prepare_offline_wal_fixture(paths: &MigrationPaths) -> OfflineWalFixture {
    let writer = Connection::open(&paths.live).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "UPDATE projects SET updated_at=updated_at+1 WHERE id='project-a'",
            [],
        )
        .unwrap();
    let (busy, _, _): (i64, i64, i64) = writer
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(busy, 0);
    assert!(paths.live_wal.exists());
    assert!(paths.live_shm.exists());
    assert_eq!(fs::metadata(&paths.live_wal).unwrap().len(), 0);
    let source = SourceSet {
        database: database_fingerprint(&writer).unwrap(),
        main: file_fingerprint(&paths.live).unwrap(),
        wal: file_fingerprint(&paths.live_wal).unwrap(),
        shm: file_fingerprint(&paths.live_shm).unwrap(),
    };
    let fixture = OfflineWalFixture {
        source,
        wal_bytes: fs::read(&paths.live_wal).unwrap(),
        shm_bytes: fs::read(&paths.live_shm).unwrap(),
    };
    assert!(fixture.wal_bytes.is_empty());
    assert!(!fixture.shm_bytes.is_empty());
    writer.close().unwrap();

    let verification = Connection::open(&paths.live).unwrap();
    assert_eq!(
        verification
            .query_row(
                "SELECT updated_at FROM projects WHERE id='project-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    verification.close().unwrap();
    fixture.materialize(paths);
    fixture.assert_materialized(paths);
    fixture
}

impl OfflineWalFixture {
    fn materialize(&self, paths: &MigrationPaths) {
        fs::write(&paths.live_wal, &self.wal_bytes).unwrap();
        fs::write(&paths.live_shm, &self.shm_bytes).unwrap();
    }

    fn assert_materialized(&self, paths: &MigrationPaths) {
        assert_eq!(fs::read(&paths.live_wal).unwrap(), self.wal_bytes);
        assert_eq!(fs::read(&paths.live_shm).unwrap(), self.shm_bytes);
        assert_eq!(file_fingerprint(&paths.live).unwrap(), self.source.main);
        assert_eq!(file_fingerprint(&paths.live_wal).unwrap(), self.source.wal);
        assert_eq!(file_fingerprint(&paths.live_shm).unwrap(), self.source.shm);
    }
}

fn prepare_populated_wal_replacement(
    paths: &MigrationPaths,
    fixture: &OfflineWalFixture,
) -> ReplacementManifest {
    fixture.assert_materialized(paths);
    assert!(fixture.source.wal.present);
    assert!(fixture.source.shm.present);
    let source_before = BackupSource {
        database: fixture.source.database.clone(),
        main: fixture.source.main.clone(),
        wal: fixture.source.wal.clone(),
    };
    let source_connection = Connection::open(&paths.live).unwrap();
    let mut replacement = Connection::open(&paths.replacement_stage).unwrap();
    Backup::new(&source_connection, &mut replacement)
        .unwrap()
        .run_to_completion(256, Duration::ZERO, None)
        .unwrap();
    source_connection.close().unwrap();
    apply_through_v13(&mut replacement).unwrap();
    replacement.close().unwrap();
    let replacement_file = file_fingerprint(&paths.replacement_stage).unwrap();
    let replacement_connection = Connection::open(&paths.replacement_stage).unwrap();
    let replacement_database = database_fingerprint(&replacement_connection).unwrap();
    replacement_connection.close().unwrap();
    fixture.materialize(paths);
    fixture.assert_materialized(paths);
    let manifest = ReplacementManifest::new(
        source_before,
        fixture.source.clone(),
        file_fingerprint(&paths.live).unwrap(),
        replacement_file,
        replacement_database,
    );
    write_stage(&paths.replacement_manifest, &manifest).unwrap();
    manifest
}

fn install_retained_pair(
    paths: &MigrationPaths,
    source: &SourceSet,
) -> super::backup::VerifiedBackup {
    let source_connection = Connection::open(&paths.live).unwrap();
    let mut backup = Connection::open(&paths.backup_final).unwrap();
    Backup::new(&source_connection, &mut backup)
        .unwrap()
        .run_to_completion(256, Duration::ZERO, None)
        .unwrap();
    source_connection.close().unwrap();
    backup.close().unwrap();
    let backup_connection = Connection::open(&paths.backup_final).unwrap();
    let backup_database = database_fingerprint(&backup_connection).unwrap();
    backup_connection.close().unwrap();
    let manifest = BackupManifest::new(
        BackupSource {
            database: source.database.clone(),
            main: source.main.clone(),
            wal: source.wal.clone(),
        },
        file_fingerprint(&paths.backup_final).unwrap(),
        backup_database,
        1,
    );
    write_stage(&paths.manifest_final, &manifest).unwrap();
    load_retained_pair(paths).unwrap()
}

fn artifact_bytes(paths: &MigrationPaths) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        fs::read(&paths.live).unwrap(),
        fs::read(&paths.live_wal).unwrap(),
        fs::read(&paths.live_shm).unwrap(),
    )
}

fn retire_prefix(paths: &MigrationPaths, source: &ReplacementManifest, prefix: &str) {
    if matches!(prefix, "wal" | "shm" | "main") {
        rename_synced(&paths.live_wal, &paths.retired_wal).unwrap();
    }
    if matches!(prefix, "shm" | "main") {
        rename_synced(&paths.live_shm, &paths.retired_shm).unwrap();
    }
    if prefix == "main" {
        rename_synced(&paths.live, &paths.retired_live).unwrap();
    }
    if matches!(prefix, "none") {
        assert!(paths.live_wal.exists());
        assert!(paths.live_shm.exists());
        assert!(paths.live.exists());
    } else {
        assert!(source.source_after_checkpoint.wal.present);
        assert!(paths.retired_wal.exists());
    }
}

fn restore_prefix(paths: &MigrationPaths, source: &ReplacementManifest, prefix: &str) {
    assert!(source.source_after_checkpoint.wal.present);
    assert!(source.source_after_checkpoint.shm.present);
    if matches!(prefix, "shm" | "wal") {
        rename_synced(&paths.retired_shm, &paths.live_shm).unwrap();
    }
    if prefix == "wal" {
        rename_synced(&paths.retired_wal, &paths.live_wal).unwrap();
    }
}

fn cleanup_prefix(paths: &MigrationPaths, prefix: &str) {
    if matches!(prefix, "shm" | "wal" | "main") {
        remove_synced(&paths.retired_shm).unwrap();
    }
    if matches!(prefix, "wal" | "main") {
        remove_synced(&paths.retired_wal).unwrap();
    }
    if prefix == "main" {
        remove_synced(&paths.retired_live).unwrap();
    }
}

fn assert_published_project(paths: &MigrationPaths) {
    assert_eq!(
        super::fingerprint::classify_path(&paths.live).unwrap(),
        SchemaClass::CurrentV13
    );
    let published = Connection::open(&paths.live).unwrap();
    assert_eq!(
        published
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "Project"
    );
    published.close().unwrap();
}

fn assert_legacy_project(paths: &MigrationPaths) {
    let restored = Connection::open(&paths.live).unwrap();
    assert_eq!(
        restored
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "Project"
    );
    restored.close().unwrap();
}

fn assert_wal_update(paths: &MigrationPaths) {
    let connection = Connection::open(&paths.live).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT updated_at FROM projects WHERE id='project-a'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    connection.close().unwrap();
}

fn simulator_state(database: &Database) -> (i64, i64, i64, i64) {
    database.connection().query_row(
        "SELECT (SELECT COUNT(*) FROM agent_sessions WHERE chat_id='chat-a'), (SELECT COUNT(*) FROM turns WHERE chat_id='chat-a'), (SELECT COUNT(*) FROM items WHERE chat_id='chat-a'), (SELECT projection_revision FROM chats WHERE id='chat-a')",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap()
}
