use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::limits::WAL_HARD_BYTES;
use super::*;
use crate::acp::{
    contract::{SageChatId, SageProjectId},
    store::load_chat_page,
};
#[cfg(windows)]
use crate::persistence::Database;

struct TestDir(
    PathBuf,
    #[allow(dead_code)] test_failpoint::AvailableBytesGuard,
);

impl TestDir {
    fn new() -> Self {
        let available_bytes = test_failpoint::override_available_bytes(u64::MAX);
        let path = std::env::temp_dir().join(format!("sage-migration-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path, available_bytes)
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

const PENDING_CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"pending\",\"newSession\":false,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":false,\"prompt\":false,\"cancel\":false,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":0,\"configCount\":0}";
const READY_CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"acp-1\",\"newSession\":true,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":true,\"prompt\":true,\"cancel\":true,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":1,\"configCount\":0}";

fn seed_retained_backup_where_creation_is_unavailable(path: &Path) {
    // Windows exercises production backup creation, including identity-bound
    // journal deletion. Unix exercises schema upgrades from a verified pair;
    // its unsupported creation path is covered separately below.
    #[cfg(windows)]
    let _ = path;
    #[cfg(not(windows))]
    {
        let mut paths = MigrationPaths::new(path).unwrap();
        if matches!(
            fingerprint::classify_path(path).unwrap(),
            SchemaClass::CurrentV16 | SchemaClass::CurrentV17
        ) {
            paths = v18_backup::paths_for_v18(&paths);
        }
        seed_retained_backup_pair(&paths);
    }
}

#[cfg(not(windows))]
fn seed_retained_backup_pair(paths: &MigrationPaths) {
    let source = open_read_only(&paths.live).unwrap();
    let source_set = backup::source_set(paths, &source).unwrap();
    let database = database_fingerprint(&source).unwrap();
    drop(source);
    assert!(!paths.backup_final.exists());
    assert!(!paths.manifest_final.exists());
    fs::copy(&paths.live, &paths.backup_final).unwrap();
    let manifest = manifest::BackupManifest::new(
        source_set,
        file_fingerprint(&paths.backup_final).unwrap(),
        database,
        1,
    );
    manifest::write_stage(&paths.manifest_final, &manifest).unwrap();
    backup::verify_retained_pair(paths).unwrap();
}

#[test]
fn populated_v16_item_rebuild_reaches_v18_with_owner_and_triggers_intact() {
    let directory = TestDir::new();
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v16(&mut source).unwrap();
    let payload = "{\"kind\":\"text\",\"payload\":{\"text\":\"Preserved\"}}";
    let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
    source.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1)", []).unwrap();
    source.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1)", []).unwrap();
    source.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session','project','chat',0,'binding',?1,'external',?2,'active','hot',0,0,0,1,1)", params!["a".repeat(64), READY_CAPABILITIES]).unwrap();
    source.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project','chat','session',0,1)", []).unwrap();
    source.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project','chat','session',1,'running',?1,1)", ["b".repeat(64)]).unwrap();
    source.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn','project','chat','session',1,0,'intent','running','correlated',0,1,1)", []).unwrap();
    source.execute("INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('item','project','chat','session','turn',1,0,'primary','text','completed','native-item',?1,?2,?3,0,1,1,1)", params![payload, payload.len() as i64, digest]).unwrap();
    source.close().unwrap();

    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
    let preserved: (String, String) = upgraded
        .query_row(
            "SELECT kind,payload_json FROM items WHERE id='item'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(preserved, ("text".into(), payload.into()));
    let rebuilt_objects: i64 = upgraded.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE tbl_name='items' AND type IN ('index','trigger') AND sql IS NOT NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(rebuilt_objects, 7);
    let dependent_foreign_keys: i64 = upgraded.query_row("SELECT COUNT(*) FROM pragma_foreign_key_list('session_activities') WHERE \"table\"='items'", [], |row| row.get(0)).unwrap();
    assert_eq!(dependent_foreign_keys, 6);
    assert!(
        upgraded
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
            .optional()
            .unwrap()
            .is_none()
    );
    assert!(
        upgraded
            .execute("DELETE FROM items WHERE id='item'", [])
            .is_err()
    );
    let new_payload = "{\"kind\":\"compactionNotice\"}";
    let new_digest = format!("{:x}", Sha256::digest(new_payload.as_bytes()));
    upgraded.execute("INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('notice','project','chat','session','turn',1,1,'primary','compactionNotice','completed','native-notice',?1,?2,?3,0,1,1,1)", params![new_payload, new_payload.len() as i64, new_digest]).unwrap();
    let paths = v18_backup::paths_for_v18(&MigrationPaths::new(&directory.database()).unwrap());
    assert_eq!(
        fingerprint::classify_path(&paths.backup_final).unwrap(),
        SchemaClass::CurrentV16
    );
}

#[test]
fn v18_markers_exceed_v17_admission_and_downgrade_spoof_fails_closed() {
    let mut connection = Connection::open_in_memory().unwrap();
    schema_upgrade::build_fresh_v18(&mut connection).unwrap();
    let markers: Vec<i64> = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version LIMIT 18")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(markers.len(), 18);
    assert!(markers.len() > 17);
    connection
        .execute("DELETE FROM schema_migrations WHERE version=18", [])
        .unwrap();
    connection.pragma_update(None, "user_version", 17).unwrap();
    assert!(fingerprint::classify_connection(&connection).is_err());
}

#[test]
fn v14_child_thread_upgrade_reaches_v18_without_losing_the_source() {
    let directory = TestDir::new();
    {
        let mut connection = Connection::open(directory.database()).unwrap();
        schema_upgrade::build_fresh_v14(&mut connection).unwrap();
        connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Preserved',1,1,1)", []).unwrap();
    }
    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
    let name: String = upgraded
        .query_row("SELECT name FROM projects WHERE id='project'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(name, "Preserved");
    let child_tables: i64 = upgraded.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('acp_child_thread_owners','acp_child_thread_bindings')",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(child_tables, 2);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let source = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&source).unwrap(),
        SchemaClass::CurrentV14
    );
}

#[test]
fn v14_child_thread_upgrade_rolls_back_atomically() {
    let directory = TestDir::new();
    {
        let mut connection = Connection::open(directory.database()).unwrap();
        schema_upgrade::build_fresh_v14(&mut connection).unwrap();
    }
    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    test_failpoint::arm("v14-to-v15-after-schema");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(
        fingerprint::classify_path(&directory.database()).unwrap(),
        SchemaClass::CurrentV14
    );
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
}

#[test]
fn v13_image_artifact_upgrade_reaches_v14_without_losing_the_source() {
    let directory = TestDir::new();
    {
        let mut connection = Connection::open(directory.database()).unwrap();
        schema_upgrade::build_fresh_v13(&mut connection).unwrap();
        connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1)", []).unwrap();
        connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1)", []).unwrap();
    }
    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
    assert_eq!(
        upgraded
            .query_row("SELECT title FROM chats WHERE id='chat'", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "Chat"
    );
    assert_eq!(upgraded.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='chat_image_artifacts'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let source = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&source).unwrap(),
        SchemaClass::CurrentV13
    );
    assert_eq!(
        source
            .query_row("SELECT title FROM chats WHERE id='chat'", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "Chat"
    );
    assert_ne!(
        fingerprint::database_fingerprint(&source)
            .unwrap()
            .schema_sha256,
        fingerprint::database_fingerprint(&upgraded)
            .unwrap()
            .schema_sha256
    );
}

#[test]
fn v13_image_artifact_upgrade_rolls_back_atomically() {
    let directory = TestDir::new();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let mut connection = Connection::open(directory.database()).unwrap();
        schema_upgrade::build_fresh_v13(&mut connection).unwrap();
    }
    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    test_failpoint::arm("v13-to-v14-after-schema");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(
        fingerprint::classify_path(&directory.database()).unwrap(),
        SchemaClass::CurrentV13
    );
    assert_eq!(
        fingerprint::classify_path(&paths.backup_final).unwrap(),
        SchemaClass::CurrentV13
    );
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
}

fn starting_session(capabilities: &str) -> OpenedDatabase {
    let connection = open_or_migrate(Path::new(":memory:")).unwrap();
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES ('project','Project',1,1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
             VALUES ('chat','project',0,'Chat',1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,
               capabilities_json,state,residency,legacy,revision,
               projection_revision,created_at,updated_at,terminal_at
             ) VALUES ('session','project','chat',0,'binding',?1,?2,
                       'starting','hot',0,0,0,1,1,NULL)",
            params!["a".repeat(64), capabilities],
        )
        .unwrap();
    connection
}

#[test]
fn starting_to_ready_accepts_exact_one_time_activation_identity_fill() {
    let connection = starting_session(PENDING_CAPABILITIES);
    connection
        .execute(
            "UPDATE agent_sessions SET state='ready', external_session_id='external',
             capabilities_json=?1, updated_at=2 WHERE id='session'",
            [READY_CAPABILITIES],
        )
        .unwrap();
    let state: (String, String) = connection
        .query_row(
            "SELECT state,external_session_id FROM agent_sessions WHERE id='session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, ("ready".into(), "external".into()));
}

#[test]
fn activation_identity_fill_is_not_repeatable() {
    let connection = starting_session(PENDING_CAPABILITIES);
    connection
        .execute(
            "UPDATE agent_sessions SET state='ready', external_session_id='external',
             capabilities_json=?1, updated_at=2 WHERE id='session'",
            [READY_CAPABILITIES],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "UPDATE agent_sessions SET external_session_id='other' WHERE id='session'",
                [],
            )
            .is_err()
    );
}

#[test]
fn activation_fill_rejects_mismatched_pending_and_state_only_edits() {
    let mismatched = starting_session("{}");
    assert!(
        mismatched
            .execute(
                "UPDATE agent_sessions SET state='ready', external_session_id='external',
                 capabilities_json=?1 WHERE id='session'",
                [READY_CAPABILITIES],
            )
            .is_err()
    );
    let state_only = starting_session(PENDING_CAPABILITIES);
    assert!(
        state_only
            .execute(
                "UPDATE agent_sessions SET state='ready' WHERE id='session'",
                [],
            )
            .is_err()
    );
}

#[test]
fn recovering_to_ready_cannot_change_activation_identity() {
    let connection = starting_session(PENDING_CAPABILITIES);
    connection
        .execute(
            "UPDATE agent_sessions SET state='ready', external_session_id='external',
             capabilities_json=?1 WHERE id='session'",
            [READY_CAPABILITIES],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET state='recovering' WHERE id='session'",
            [],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "UPDATE agent_sessions SET state='ready', external_session_id='other',
                 capabilities_json='{}' WHERE id='session'",
                [],
            )
            .is_err()
    );
}

#[test]
fn in_memory_creation_reaches_exact_v19() {
    let connection = open_or_migrate(Path::new(":memory:")).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
}

#[test]
fn populated_v11_tabs_upgrade_to_v12_without_losing_chat_attachment() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v11(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV11
    );
    connection
        .execute_batch(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES ('project','Project',1,1,1);
             INSERT INTO workbench_tabs(id,project_id,kind,title,position,pinned,dirty,status,metadata_json,created_at,updated_at)
             VALUES ('chat','project','chat','Chat',0,0,0,'idle','{}',1,1),
                    ('settings','project','settings','Settings',1,0,0,NULL,'{}',1,1);
             INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
             VALUES ('chat','project',0,'Chat',1,1);
             INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
             VALUES ('project','chat','chat',1);",
        )
        .unwrap();
    drop(connection);

    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
    let tabs: Vec<(String, String)> = upgraded
        .prepare("SELECT id, kind FROM workbench_tabs ORDER BY position")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        tabs,
        vec![
            ("chat".into(), "chat".into()),
            ("settings".into(), "settings".into())
        ]
    );
    let attachment_count: i64 = upgraded
        .query_row(
            "SELECT COUNT(*) FROM chat_tabs WHERE tab_id='chat'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attachment_count, 1);
    let fk_errors: i64 = upgraded
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fk_errors, 0);
    upgraded.execute(
        "INSERT INTO workbench_tabs(id,project_id,kind,title,position,pinned,dirty,status,metadata_json,created_at,updated_at)
         VALUES ('browser-1','project','browser','Browser',2,0,0,NULL,'{}',2,2)",
        [],
    ).unwrap();
}

#[test]
fn fresh_open_retains_turn_usage_snapshot_schema_at_version_22() {
    let connection = open_or_migrate(Path::new(":memory:")).unwrap();
    let version: i64 = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 22);
    let allow_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name='server_requests'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        allow_sql.contains("asked")
            && allow_sql.contains("allowPending")
            && allow_sql.contains("resolvedAllowed")
            && allow_sql.contains("cancelled"),
        "server_requests CHECK must admit decisions and cancellation: {allow_sql}"
    );
}

#[test]
fn v10_usage_items_backfill_into_turn_usage_snapshots() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v10(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV10
    );
    let payload = "{\"kind\":\"usage\",\"payload\":{\"inputTokens\":10,\"outputTokens\":4,\"contextWindow\":272000}}";
    let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root','project','C:/project','c:/project',0)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat','project',0,'Chat',0,1,1)", []).unwrap();
    connection.execute(
        "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session','project','chat',0,'binding',?1,'external',?2,'active','hot',0,0,0,1,1)",
        params!["a".repeat(64), READY_CAPABILITIES],
    ).unwrap();
    connection.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project','chat','session',0,1)", []).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project','chat','session',1,'running',?1,1)", ["b".repeat(64)]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn','project','chat','session',1,0,'intent','running','correlated',0,1,1)", []).unwrap();
    connection.execute(
        "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('usage','project','chat','session','turn',1,0,'primary','usage','completed','usage',?1,?2,?3,0,1,1,1)",
        params![payload, payload.len() as i64, digest],
    ).unwrap();
    connection
        .execute(
            "UPDATE acp_session_usage SET item_count=1,item_bytes=?1,payload_bytes=?1 WHERE session_id='session'",
            [payload.len() as i64],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE acp_project_usage SET item_count=1,payload_bytes=?1 WHERE project_id='project'",
            [payload.len() as i64],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE acp_store_usage SET item_count=1,item_bytes=?1,shared_payload_bytes=?1 WHERE singleton=1",
            [payload.len() as i64],
        )
        .unwrap();
    connection.close().unwrap();
    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let migrated = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV22
    );
    let snapshot: (i64, i64, Option<i64>) = migrated
        .query_row(
            "SELECT input_tokens,output_tokens,context_window FROM acp_turn_usage_snapshots WHERE turn_id='turn'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(snapshot, (10, 4, Some(272_000)));
    let items: i64 = migrated
        .query_row("SELECT COUNT(*) FROM items WHERE kind='usage'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(items, 0);
    let page = load_chat_page(
        &migrated,
        SageProjectId::parse("project").unwrap(),
        SageChatId::parse("chat").unwrap(),
        None,
    )
    .unwrap();
    let turn = page.turns.as_slice().first().unwrap();
    assert_eq!(turn.items.as_slice().len(), 0);
    assert_eq!(
        turn.usage.as_ref().map(|usage| (
            usage.input_tokens,
            usage.output_tokens,
            usage.context_window,
        )),
        Some((10, 4, Some(272_000)))
    );
    serde_json::to_value(&page).unwrap();
}

#[test]
fn v10_with_empty_legacy_session_events_migrates_to_v12() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v10(&mut connection).unwrap();
    connection
        .execute_batch(fingerprint::V10_LEGACY_SESSION_EVENTS)
        .unwrap();
    assert!(fingerprint::classify_connection(&connection).is_ok());
    connection.close().unwrap();

    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let migrated = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV22
    );
    let legacy_objects: i64 = migrated
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE name IN ('session_events', 'idx_session_events_order')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_objects, 0);
}

#[test]
fn v10_with_nonempty_legacy_session_events_is_rejected() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v10(&mut connection).unwrap();
    connection
        .execute_batch(fingerprint::V10_LEGACY_SESSION_EVENTS)
        .unwrap();
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_events(id,session_id,sequence,kind,payload_json,created_at)
             VALUES('event','missing',0,'message','{}',1)",
            [],
        )
        .unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV10LegacySessionEvents
    );
    connection.close().unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
}

#[cfg(windows)]
#[test]
fn exact_v1_v2_v3_sources_migrate_through_v7() {
    for version in 1..=3 {
        let directory = TestDir::new();
        let connection = Connection::open(directory.database()).unwrap();
        connection.execute_batch(INITIAL).unwrap();
        if version >= 2 {
            connection.execute_batch(V2).unwrap();
        }
        if version >= 3 {
            connection.execute_batch(V3).unwrap();
        }
        if version == 2 {
            connection.pragma_update(None, "user_version", 2).unwrap();
        }
        connection.close().unwrap();
        let migrated = open_or_migrate(&directory.database()).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&migrated).unwrap(),
            SchemaClass::CurrentV22,
            "source version {version}"
        );
    }
}

#[cfg(windows)]
#[test]
fn exact_v5_source_migrates_to_bounded_resource_trash_intents() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(V2).unwrap();
    connection.execute_batch(V3).unwrap();
    project_migration::migrate(&mut connection).unwrap();
    apply_v5(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV5
    );
    connection.close().unwrap();

    let migrated = open_or_migrate(&directory.database()).unwrap();

    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV22
    );
    let columns: i64 = migrated
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_xinfo('resource_trash_intents')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(columns, 13);
    drop(migrated);

    let paths = MigrationPaths::new(&directory.database()).unwrap();
    backup::verify_retained_pair(&paths).unwrap();
    let original = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&original).unwrap(),
        SchemaClass::CurrentV5
    );
}

#[cfg(windows)]
#[test]
fn exact_v6_source_upgrades_to_v7_without_losing_epoch_children() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(V2).unwrap();
    connection.execute_batch(V3).unwrap();
    project_migration::migrate(&mut connection).unwrap();
    apply_v5(&mut connection).unwrap();
    apply_v6(&mut connection).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV6
    );
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES ('project','Project',1,1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
             VALUES ('chat','project',0,'Chat',1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,
               capabilities_json,state,residency,legacy,revision,
               projection_revision,created_at,updated_at,terminal_at
             ) VALUES ('session','project','chat',0,'binding',?1,?2,
                       'starting','hot',0,0,0,1,1,NULL)",
            params!["a".repeat(64), PENDING_CAPABILITIES],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,
               started_at,terminal_at,exit_kind
             ) VALUES ('project','chat','session',1,'starting',?1,1,NULL,NULL)",
            ["b".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
               state,delivery_state,revision,created_at,updated_at,terminal_at
             ) VALUES ('turn','project','chat','session',1,0,'intent','queued',
                       'notWritten',0,1,1,NULL)",
            [],
        )
        .unwrap();
    connection.close().unwrap();

    let migrated = open_or_migrate(&directory.database()).unwrap();

    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV22
    );
    assert_eq!(
        migrated
            .query_row("SELECT COUNT(*) FROM turns WHERE id='turn'", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        1
    );
    assert_eq!(
        migrated
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        22
    );
    assert!(
        migrated
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
            .optional()
            .unwrap()
            .is_none()
    );
    drop(migrated);

    let paths = MigrationPaths::new(&directory.database()).unwrap();
    backup::verify_retained_pair(&paths).unwrap();
    let original = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&original).unwrap(),
        SchemaClass::CurrentV6
    );
}

#[cfg(windows)]
#[test]
fn current_structural_upgrade_preserves_an_existing_verified_backup() {
    for (source_schema, label) in [
        (SchemaClass::CurrentV5, "preserved-v5"),
        (SchemaClass::CurrentV6, "preserved-v6"),
    ] {
        let directory = TestDir::new();
        create_current_schema(&directory.database(), source_schema, label);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        {
            let _lock = MigrationLock::acquire(paths.parent()).unwrap();
            backup::ensure(&paths).unwrap();
        }
        let backup_before = fs::read(&paths.backup_final).unwrap();
        let manifest_before = fs::read(&paths.manifest_final).unwrap();

        let migrated = open_or_migrate(&directory.database()).unwrap();
        let item: (String, i64) = migrated
            .query_row("SELECT id, ordinal FROM items", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(item, ("event-1".into(), 1));
        drop(migrated);

        backup::verify_retained_pair(&paths).unwrap();
        assert_eq!(backup_before, fs::read(&paths.backup_final).unwrap());
        assert_eq!(manifest_before, fs::read(&paths.manifest_final).unwrap());
        let original = open_read_only(&paths.backup_final).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&original).unwrap(),
            source_schema
        );
    }
}

#[cfg(windows)]
#[test]
fn v5_to_v7_failure_after_v6_work_rolls_back_and_retry_preserves_original_backup() {
    let directory = TestDir::new();
    create_current_schema(
        &directory.database(),
        SchemaClass::CurrentV5,
        "atomic-retry",
    );
    let paths = MigrationPaths::new(&directory.database()).unwrap();

    test_failpoint::arm("v5-to-v7-after-v6");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!test_failpoint::is_armed());
    assert_eq!(
        classify_path(&directory.database()).unwrap(),
        SchemaClass::CurrentV5
    );
    backup::verify_retained_pair(&paths).unwrap();
    let backup_after_failure = fs::read(&paths.backup_final).unwrap();
    let manifest_after_failure = fs::read(&paths.manifest_final).unwrap();

    let migrated = open_or_migrate(&directory.database()).unwrap();

    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV22
    );
    let item: (String, i64) = migrated
        .query_row("SELECT id, ordinal FROM items", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(item, ("event-1".into(), 1));
    drop(migrated);
    backup::verify_retained_pair(&paths).unwrap();
    assert_eq!(backup_after_failure, fs::read(&paths.backup_final).unwrap());
    assert_eq!(
        manifest_after_failure,
        fs::read(&paths.manifest_final).unwrap()
    );
    let original = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&original).unwrap(),
        SchemaClass::CurrentV5
    );
}

#[cfg(windows)]
#[test]
fn wal_v5_retry_reuses_original_backup_after_physical_checkpoint() {
    let directory = TestDir::new();
    create_current_schema(&directory.database(), SchemaClass::CurrentV5, "before-wal");
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let reader = Connection::open(&paths.live).unwrap();
    reader.pragma_update(None, "journal_mode", "WAL").unwrap();
    reader.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    reader.execute_batch("BEGIN").unwrap();
    assert_eq!(
        reader
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "before-wal"
    );
    let writer = Connection::open(&paths.live).unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "UPDATE projects SET name='committed-in-wal' WHERE id='project-a'",
            [],
        )
        .unwrap();
    writer.close().unwrap();
    let wal_with_committed_frames = file_fingerprint(&paths.live_wal).unwrap();
    assert!(wal_with_committed_frames.present && wal_with_committed_frames.bytes > 0);

    test_failpoint::arm("v5-to-v7-after-v6");
    assert!(open_or_migrate(&paths.live).is_err());
    assert!(!test_failpoint::is_armed());
    assert_eq!(
        probe::classify_live(&paths).unwrap(),
        SchemaClass::CurrentV5
    );
    backup::verify_retained_pair(&paths).unwrap();
    let backup_after_failure = fs::read(&paths.backup_final).unwrap();
    let manifest_after_failure = fs::read(&paths.manifest_final).unwrap();
    let physical_before_checkpoint = (
        file_fingerprint(&paths.live).unwrap(),
        file_fingerprint(&paths.live_wal).unwrap(),
    );

    reader.execute_batch("ROLLBACK").unwrap();
    reader.close().unwrap();
    let checkpoint = Connection::open(&paths.live).unwrap();
    let (busy, _log, _checkpointed): (i64, i64, i64) = checkpoint
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(busy, 0);
    checkpoint.close().unwrap();
    let physical_after_checkpoint = (
        file_fingerprint(&paths.live).unwrap(),
        file_fingerprint(&paths.live_wal).unwrap(),
    );
    assert_ne!(physical_before_checkpoint, physical_after_checkpoint);
    assert!(
        !physical_after_checkpoint.1.present || physical_after_checkpoint.1.bytes == 0,
        "checkpoint must consume the committed WAL frames"
    );
    let checkpointed = open_read_only(&paths.live).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&checkpointed).unwrap(),
        SchemaClass::CurrentV5
    );
    assert_eq!(
        checkpointed
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "committed-in-wal"
    );
    drop(checkpointed);

    let migrated = open_or_migrate(&paths.live).unwrap();

    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV22
    );
    assert_eq!(
        migrated
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "committed-in-wal"
    );
    drop(migrated);
    backup::verify_retained_pair(&paths).unwrap();
    assert_eq!(backup_after_failure, fs::read(&paths.backup_final).unwrap());
    assert_eq!(
        manifest_after_failure,
        fs::read(&paths.manifest_final).unwrap()
    );
    let original = open_read_only(&paths.backup_final).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&original).unwrap(),
        SchemaClass::CurrentV5
    );
    assert_eq!(
        original
            .query_row(
                "SELECT name FROM projects WHERE id='project-a'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
        "committed-in-wal"
    );
}

#[cfg(windows)]
#[test]
fn tampered_v6_intermediate_with_v5_backup_is_rejected_unchanged() {
    let directory = TestDir::new();
    create_current_schema(&directory.database(), SchemaClass::CurrentV5, "original-v5");
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        backup::ensure(&paths).unwrap();
    }
    let backup_before = fs::read(&paths.backup_final).unwrap();
    let manifest_before = fs::read(&paths.manifest_final).unwrap();
    let mut intermediate = Connection::open(&paths.live).unwrap();
    apply_v6(&mut intermediate).unwrap();
    intermediate
        .execute(
            "UPDATE projects SET name='tampered-v6' WHERE id='project-a'",
            [],
        )
        .unwrap();
    intermediate.close().unwrap();
    let live_file_before = file_fingerprint(&paths.live).unwrap();
    let live_database_before = {
        let connection = open_read_only(&paths.live).unwrap();
        database_fingerprint(&connection).unwrap()
    };

    assert!(open_or_migrate(&paths.live).is_err());

    assert_eq!(live_file_before, file_fingerprint(&paths.live).unwrap());
    let unchanged = open_read_only(&paths.live).unwrap();
    assert_eq!(
        database_fingerprint(&unchanged).unwrap(),
        live_database_before
    );
    let name: String = unchanged
        .query_row(
            "SELECT name FROM projects WHERE id='project-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "tampered-v6");
    assert_eq!(backup_before, fs::read(&paths.backup_final).unwrap());
    assert_eq!(manifest_before, fs::read(&paths.manifest_final).unwrap());
    backup::verify_retained_pair(&paths).unwrap();
}

#[cfg(windows)]
#[test]
fn current_v5_upgrade_rejects_missing_tampered_and_mismatched_backups_unchanged() {
    for corruption in [
        "missing-manifest",
        "missing-backup",
        "tampered-manifest",
        "tampered-backup",
    ] {
        let directory = TestDir::new();
        create_current_schema(&directory.database(), SchemaClass::CurrentV5, corruption);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        {
            let _lock = MigrationLock::acquire(paths.parent()).unwrap();
            backup::ensure(&paths).unwrap();
        }
        if corruption == "missing-manifest" {
            fs::remove_file(&paths.manifest_final).unwrap();
        } else if corruption == "missing-backup" {
            fs::remove_file(&paths.backup_final).unwrap();
        } else if corruption == "tampered-manifest" {
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(&paths.manifest_final).unwrap()).unwrap();
            manifest["source"]["database"]["logicalSha256"] =
                serde_json::Value::String("0".repeat(64));
            fs::write(
                &paths.manifest_final,
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        } else {
            use std::io::Write;
            let mut backup = fs::OpenOptions::new()
                .append(true)
                .open(&paths.backup_final)
                .unwrap();
            backup.write_all(b"tampered").unwrap();
            backup.sync_all().unwrap();
        }
        assert_current_v5_remains_unchanged(&directory.database(), corruption);
    }

    let source = TestDir::new();
    let foreign = TestDir::new();
    create_current_schema(&source.database(), SchemaClass::CurrentV5, "source");
    create_current_schema(&foreign.database(), SchemaClass::CurrentV5, "foreign");
    let source_paths = MigrationPaths::new(&source.database()).unwrap();
    let foreign_paths = MigrationPaths::new(&foreign.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(source_paths.parent()).unwrap();
        backup::ensure(&source_paths).unwrap();
    }
    fs::copy(&source_paths.backup_final, &foreign_paths.backup_final).unwrap();
    fs::copy(&source_paths.manifest_final, &foreign_paths.manifest_final).unwrap();
    assert_current_v5_remains_unchanged(&foreign.database(), "foreign");
}

#[test]
fn fresh_current_v12_open_creates_no_retained_backup() {
    let directory = TestDir::new();
    drop(open_or_migrate(&directory.database()).unwrap());
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    assert!(!paths.backup_final.exists());
    assert!(!paths.manifest_final.exists());

    drop(open_or_migrate(&directory.database()).unwrap());

    assert!(!paths.backup_final.exists());
    assert!(!paths.manifest_final.exists());
}

#[test]
fn populated_v9_upgrades_to_v12_without_losing_project_and_tab() {
    let directory = TestDir::new();
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v9(&mut source).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&source).unwrap(),
        SchemaClass::CurrentV9
    );
    source.execute_batch(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
         VALUES ('project','V9 project',1,1,1);
         INSERT INTO workbench_tabs(id,project_id,kind,title,position,pinned,dirty,status,metadata_json,created_at,updated_at)
         VALUES ('settings','project','settings','V9 settings',0,0,0,NULL,'{}',1,1);",
    ).unwrap();
    source.close().unwrap();

    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&upgraded).unwrap(),
        SchemaClass::CurrentV22
    );
    let versions: Vec<i64> = upgraded
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        versions,
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22
        ]
    );
    let project_name: String = upgraded
        .query_row("SELECT name FROM projects WHERE id='project'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(project_name, "V9 project");
    let tab: (String, String, i64) = upgraded
        .query_row(
            "SELECT kind,title,position FROM workbench_tabs WHERE id='settings'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(tab, ("settings".into(), "V9 settings".into(), 0));
    let fk_errors: i64 = upgraded
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fk_errors, 0);
    upgraded.execute(
        "INSERT INTO workbench_tabs(id,project_id,kind,title,position,pinned,dirty,status,metadata_json,created_at,updated_at)
         VALUES ('browser-1','project','browser','Browser',1,0,0,NULL,'{}',2,2)",
        [],
    ).unwrap();
}

#[test]
fn fresh_schema_equals_a_v6_schema_upgraded_in_place() {
    let fresh = open_or_migrate(Path::new(":memory:")).unwrap();
    let fresh_fingerprint = database_fingerprint(&fresh).unwrap();
    let mut upgraded = Connection::open_in_memory().unwrap();
    upgraded.execute_batch(INITIAL).unwrap();
    upgraded.execute_batch(V2).unwrap();
    upgraded.execute_batch(V3).unwrap();
    project_migration::migrate(&mut upgraded).unwrap();
    apply_v5(&mut upgraded).unwrap();
    apply_v6(&mut upgraded).unwrap();
    schema_upgrade::apply_through_v18(&mut upgraded).unwrap();
    workspace_changes::apply(&mut upgraded).unwrap();
    title_provenance::apply(&mut upgraded).unwrap();
    session_scope::apply(&mut upgraded).unwrap();
    config_choices::apply(&mut upgraded).unwrap();
    let upgraded_fingerprint = database_fingerprint(&upgraded).unwrap();

    assert_eq!(fresh_fingerprint.schema, SchemaClass::CurrentV22);
    assert_eq!(
        fresh_fingerprint.schema_sha256,
        upgraded_fingerprint.schema_sha256
    );
}

#[test]
fn v12_queue_upgrade_adds_only_the_queue_table_and_is_idempotent() {
    let directory = TestDir::new();
    let mut source = Connection::open(directory.database()).unwrap();
    schema_upgrade::build_fresh_v12(&mut source).unwrap();
    source.execute(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('project','Preserved',1,1,1)",
        [],
    ).unwrap();
    source.close().unwrap();

    seed_retained_backup_where_creation_is_unavailable(&directory.database());
    let upgraded = open_or_migrate(&directory.database()).unwrap();
    let project_name: String = upgraded
        .query_row("SELECT name FROM projects WHERE id='project'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(project_name, "Preserved");
    let queue_tables: i64 = upgraded
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='chat_queued_prompts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(queue_tables, 1);
    upgraded.connection.close().unwrap();

    let reopened = open_or_migrate(&directory.database()).unwrap();
    let queue_tables: i64 = reopened
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='chat_queued_prompts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(queue_tables, 1);
}

#[test]
fn faux_v5_is_rejected_without_changing_database_bytes() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-a", false);
    let connection = Connection::open(directory.database()).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (5, 0)",
            [],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 5).unwrap();
    drop(connection);
    let metadata_before = fs::metadata(directory.database()).unwrap();
    let wal = directory.0.join("sage.db-wal");
    let shm = directory.0.join("sage.db-shm");
    let wal_before = wal.exists();
    let shm_before = shm.exists();
    let before = file_fingerprint(&directory.database()).unwrap();
    assert!(classify_path(&directory.database()).is_err());
    let after = file_fingerprint(&directory.database()).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        metadata_before.modified().unwrap(),
        fs::metadata(directory.database())
            .unwrap()
            .modified()
            .unwrap()
    );
    assert_eq!(wal_before, wal.exists());
    assert_eq!(shm_before, shm.exists());
}

#[test]
fn faux_v7_marker_on_a_v6_schema_is_rejected_without_changing_bytes() {
    let directory = TestDir::new();
    let mut connection = Connection::open(directory.database()).unwrap();
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(V2).unwrap();
    connection.execute_batch(V3).unwrap();
    project_migration::migrate(&mut connection).unwrap();
    apply_v5(&mut connection).unwrap();
    apply_v6(&mut connection).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (7, 0)",
            [],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 7).unwrap();
    connection.close().unwrap();
    let before = file_fingerprint(&directory.database()).unwrap();

    assert!(classify_path(&directory.database()).is_err());

    assert_eq!(before, file_fingerprint(&directory.database()).unwrap());
}

#[test]
fn bounded_probe_reads_committed_wal_without_touching_any_source_file() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "wal-source", false);
    let writer = Connection::open(directory.database()).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "UPDATE projects SET name='committed-in-wal' WHERE id='project-a'",
            [],
        )
        .unwrap();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let before = [
        file_fingerprint(&paths.live).unwrap(),
        file_fingerprint(&paths.live_wal).unwrap(),
        file_fingerprint(&paths.live_shm).unwrap(),
    ];
    let modified = [
        fs::metadata(&paths.live).unwrap().modified().unwrap(),
        fs::metadata(&paths.live_wal).unwrap().modified().unwrap(),
        fs::metadata(&paths.live_shm).unwrap().modified().unwrap(),
    ];
    let _lock = MigrationLock::acquire(paths.parent()).unwrap();
    assert_eq!(probe::classify_live(&paths).unwrap(), SchemaClass::LegacyV4);
    assert_eq!(
        before,
        [
            file_fingerprint(&paths.live).unwrap(),
            file_fingerprint(&paths.live_wal).unwrap(),
            file_fingerprint(&paths.live_shm).unwrap(),
        ]
    );
    assert_eq!(
        modified,
        [
            fs::metadata(&paths.live).unwrap().modified().unwrap(),
            fs::metadata(&paths.live_wal).unwrap().modified().unwrap(),
            fs::metadata(&paths.live_shm).unwrap().modified().unwrap(),
        ]
    );
    drop(writer);
}

#[test]
fn unsupported_schema_committed_in_wal_is_rejected_without_source_changes() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "wal-schema", false);
    let writer = Connection::open(directory.database()).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute("CREATE TABLE injected(value TEXT)", [])
        .unwrap();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let before = [
        file_fingerprint(&paths.live).unwrap(),
        file_fingerprint(&paths.live_wal).unwrap(),
        file_fingerprint(&paths.live_shm).unwrap(),
    ];
    let modified = [
        fs::metadata(&paths.live).unwrap().modified().unwrap(),
        fs::metadata(&paths.live_wal).unwrap().modified().unwrap(),
        fs::metadata(&paths.live_shm).unwrap().modified().unwrap(),
    ];
    let _lock = MigrationLock::acquire(paths.parent()).unwrap();
    assert!(probe::classify_live(&paths).is_err());
    assert_eq!(
        before,
        [
            file_fingerprint(&paths.live).unwrap(),
            file_fingerprint(&paths.live_wal).unwrap(),
            file_fingerprint(&paths.live_shm).unwrap(),
        ]
    );
    assert_eq!(
        modified,
        [
            fs::metadata(&paths.live).unwrap().modified().unwrap(),
            fs::metadata(&paths.live_wal).unwrap().modified().unwrap(),
            fs::metadata(&paths.live_shm).unwrap().modified().unwrap(),
        ]
    );
    drop(writer);
}

#[test]
fn orphan_probe_artifact_blocks_without_overwrite() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "orphan-probe", false);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    fs::write(&paths.probe_main, b"foreign-probe").unwrap();
    let before = fs::read(&paths.probe_main).unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(before, fs::read(&paths.probe_main).unwrap());
    assert_eq!(
        classify_path(&directory.database()).unwrap(),
        SchemaClass::LegacyV4
    );
}

#[test]
fn mismatched_probe_snapshot_blocks_and_preserves_source() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "mismatched-probe", false);
    let writer = Connection::open(directory.database()).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "UPDATE projects SET name='wal-data' WHERE id='project-a'",
            [],
        )
        .unwrap();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let main = file_fingerprint(&paths.live).unwrap();
    let wal = file_fingerprint(&paths.live_wal).unwrap();
    let shm = file_fingerprint(&paths.live_shm).unwrap();
    let _lock = MigrationLock::acquire(paths.parent()).unwrap();
    platform::copy_bounded_create_new(&paths.live, &paths.probe_main, LIVE_SQLITE_BYTES).unwrap();
    fs::write(&paths.probe_wal, b"not-the-source-wal").unwrap();
    assert!(probe::verify_snapshot(&paths, &main, &wal, &shm).is_err());
    assert_eq!(main, file_fingerprint(&paths.live).unwrap());
    assert_eq!(wal, file_fingerprint(&paths.live_wal).unwrap());
    assert_eq!(shm, file_fingerprint(&paths.live_shm).unwrap());
    fs::remove_file(&paths.probe_wal).unwrap();
    fs::remove_file(&paths.probe_main).unwrap();
    drop(writer);
}

#[test]
fn identity_bound_probe_crash_prefixes_converge() {
    for prefix in [
        "manifest",
        "partial-main",
        "partial-wal",
        "complete-with-shm",
    ] {
        let (directory, writer, paths) = create_wal_source(prefix);
        let directory_path = directory.0.clone();
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        probe::write_current_identity(&paths).unwrap();
        if prefix != "manifest" {
            platform::copy_bounded_create_new(&paths.live, &paths.probe_main, LIVE_SQLITE_BYTES)
                .unwrap();
        }
        if prefix == "partial-main" {
            let length = fs::metadata(&paths.probe_main).unwrap().len();
            fs::OpenOptions::new()
                .write(true)
                .open(&paths.probe_main)
                .unwrap()
                .set_len(length / 2)
                .unwrap();
        }
        if matches!(prefix, "partial-wal" | "complete-with-shm") {
            platform::copy_bounded_create_new(&paths.live_wal, &paths.probe_wal, WAL_HARD_BYTES)
                .unwrap();
        }
        if prefix == "partial-wal" {
            let length = fs::metadata(&paths.probe_wal).unwrap().len();
            fs::OpenOptions::new()
                .write(true)
                .open(&paths.probe_wal)
                .unwrap()
                .set_len(length / 2)
                .unwrap();
        }
        if prefix == "complete-with-shm" {
            fs::write(&paths.probe_shm, vec![0_u8; 32_768]).unwrap();
        }
        assert!(!paths.has_replacement_artifact());
        assert_eq!(probe::classify_live(&paths).unwrap(), SchemaClass::LegacyV4);
        assert_probe_clean(&paths);
        drop(writer);
        drop(_lock);
        drop(directory);
        assert!(!directory_path.exists());
    }
}

#[test]
fn foreign_probe_identity_is_preserved_and_blocks() {
    let (directory, writer, paths) = create_wal_source("foreign-probe-identity");
    let directory_path = directory.0.clone();
    let _lock = MigrationLock::acquire(paths.parent()).unwrap();
    probe::write_current_identity(&paths).unwrap();
    let manifest = fs::read_to_string(&paths.probe_manifest).unwrap();
    let foreign = manifest.replace(
        "sage-ade-0006a-migration-probe-v1",
        "foreign-ade-0006a-migration-probe-v1",
    );
    assert_ne!(manifest, foreign);
    fs::write(&paths.probe_manifest, foreign.as_bytes()).unwrap();
    let source_before = (
        file_fingerprint(&paths.live).unwrap(),
        file_fingerprint(&paths.live_wal).unwrap(),
        file_fingerprint(&paths.live_shm).unwrap(),
    );
    assert!(probe::classify_live(&paths).is_err());
    assert_eq!(foreign.as_bytes(), fs::read(&paths.probe_manifest).unwrap());
    assert_eq!(
        source_before,
        (
            file_fingerprint(&paths.live).unwrap(),
            file_fingerprint(&paths.live_wal).unwrap(),
            file_fingerprint(&paths.live_shm).unwrap(),
        )
    );
    fs::remove_file(&paths.probe_manifest).unwrap();
    drop(writer);
    drop(_lock);
    drop(directory);
    assert!(!directory_path.exists());
}

#[cfg(windows)]
#[test]
fn committed_wal_data_is_included_in_migration() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "before-wal", false);
    let writer = Connection::open(directory.database()).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "UPDATE projects SET name='committed-in-wal' WHERE id='project-a'",
            [],
        )
        .unwrap();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let _lock = MigrationLock::acquire(paths.parent()).unwrap();
    assert_eq!(probe::classify_live(&paths).unwrap(), SchemaClass::LegacyV4);
    let backup = ensure(&paths).unwrap();
    drop(writer);
    build_intent::begin(&paths, &backup).unwrap();
    let intent = build_intent::read_intent(&paths).unwrap();
    build_replacement(&paths, &backup, &intent.nonce).unwrap();
    let source_after = checkpoint_source(&paths, &backup).unwrap();
    let replacement = file_fingerprint(&paths.replacement_stage).unwrap();
    let replacement_connection = open_read_only(&paths.replacement_stage).unwrap();
    let replacement_database = database_fingerprint(&replacement_connection).unwrap();
    drop(replacement_connection);
    let manifest = ReplacementManifest::new(
        backup.source,
        source_after,
        backup.backup_file,
        replacement,
        replacement_database,
    );
    write_stage(&paths.replacement_manifest, &manifest).unwrap();
    assert!(recover_or_publish(&paths).unwrap());
    let migrated = Connection::open(&paths.live).unwrap();
    // The replacement publishes v18; v19 is applied in place at open.
    assert_eq!(
        fingerprint::classify_connection(&migrated).unwrap(),
        SchemaClass::CurrentV18
    );
    let name: String = migrated
        .query_row(
            "SELECT name FROM projects WHERE id='project-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "committed-in-wal");
}

#[cfg(windows)]
#[test]
fn replacement_build_intent_crash_prefixes_converge() {
    for prefix in [
        "intent-only",
        "partial-stage",
        "complete-stage",
        "checkpointed-stage",
        "both-manifests",
    ] {
        let directory = TestDir::new();
        create_v4(&directory.database(), prefix, true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        {
            let _lock = MigrationLock::acquire(paths.parent()).unwrap();
            let backup = ensure(&paths).unwrap();
            build_intent::begin(&paths, &backup).unwrap();
            let intent = build_intent::read_intent(&paths).unwrap();
            if prefix == "partial-stage" {
                let anchor = stage_owner::create_bound_stage(
                    &paths.replacement_stage,
                    &paths.replacement_journal,
                    &paths.replacement_stage_owner,
                    &intent.nonce,
                )
                .unwrap();
                fs::write(&paths.replacement_stage, b"bounded-partial-stage").unwrap();
                anchor.verify_path(&paths.replacement_stage).unwrap();
            }
            if matches!(
                prefix,
                "complete-stage" | "checkpointed-stage" | "both-manifests"
            ) {
                build_replacement(&paths, &backup, &intent.nonce).unwrap();
            }
            let source_after = if matches!(prefix, "checkpointed-stage" | "both-manifests") {
                Some(checkpoint_source(&paths, &backup).unwrap())
            } else {
                None
            };
            if prefix == "both-manifests" {
                let replacement = file_fingerprint(&paths.replacement_stage).unwrap();
                let replacement_connection = open_read_only(&paths.replacement_stage).unwrap();
                let replacement_database = database_fingerprint(&replacement_connection).unwrap();
                drop(replacement_connection);
                let manifest = ReplacementManifest::new(
                    backup.source,
                    source_after.unwrap(),
                    backup.backup_file,
                    replacement,
                    replacement_database,
                );
                write_stage(&paths.replacement_manifest, &manifest).unwrap();
            }
        }
        let migrated = open_or_migrate(&directory.database()).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&migrated).unwrap(),
            SchemaClass::CurrentV22,
            "prefix {prefix}"
        );
        drop(migrated);
        assert!(!paths.replacement_build_intent.exists(), "prefix {prefix}");
        assert!(!paths.replacement_stage.exists(), "prefix {prefix}");
        assert!(!paths.replacement_manifest.exists(), "prefix {prefix}");
        assert!(!paths.replacement_journal.exists(), "prefix {prefix}");
        assert!(paths.backup_final.exists(), "prefix {prefix}");
        assert!(paths.manifest_final.exists(), "prefix {prefix}");
    }
}

#[cfg(windows)]
#[test]
fn foreign_or_mismatched_build_intent_is_preserved_and_blocks() {
    for corruption in ["foreign", "mismatched-source"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), corruption, false);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        {
            let _lock = MigrationLock::acquire(paths.parent()).unwrap();
            let backup = ensure(&paths).unwrap();
            build_intent::begin(&paths, &backup).unwrap();
        }
        let original = fs::read_to_string(&paths.replacement_build_intent).unwrap();
        let corrupted = if corruption == "foreign" {
            original.replace(
                "sage-ade-0006a-schema-005-v1",
                "foreign-ade-0006a-schema-005-v1",
            )
        } else {
            let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
            value["sourceDatabase"]["logicalSha256"] = serde_json::Value::String("0".repeat(64));
            format!("{}\n", serde_json::to_string(&value).unwrap())
        };
        assert_ne!(original, corrupted);
        fs::write(&paths.replacement_build_intent, corrupted.as_bytes()).unwrap();
        let live_before = file_fingerprint(&paths.live).unwrap();
        assert!(open_or_migrate(&directory.database()).is_err());
        assert_eq!(
            corrupted.as_bytes(),
            fs::read(&paths.replacement_build_intent).unwrap(),
            "corruption {corruption}"
        );
        assert_eq!(live_before, file_fingerprint(&paths.live).unwrap());
        assert!(!paths.replacement_stage.exists());
        assert!(!paths.replacement_manifest.exists());
    }
}

#[cfg(windows)]
#[test]
fn retained_backup_from_another_database_is_a_non_destructive_blocker() {
    let first = TestDir::new();
    let second = TestDir::new();
    create_v4(&first.database(), "project-a", false);
    create_v4(&second.database(), "project-b", false);
    let first_paths = MigrationPaths::new(&first.database()).unwrap();
    let second_paths = MigrationPaths::new(&second.database()).unwrap();
    let _lock = MigrationLock::acquire(first_paths.parent()).unwrap();
    ensure(&first_paths).unwrap();
    fs::copy(&first_paths.backup_final, &second_paths.backup_final).unwrap();
    fs::copy(&first_paths.manifest_final, &second_paths.manifest_final).unwrap();
    let backup_before = file_fingerprint(&second_paths.backup_final).unwrap();
    assert!(ensure(&second_paths).is_err());
    assert_eq!(
        backup_before,
        file_fingerprint(&second_paths.backup_final).unwrap()
    );
    assert_eq!(
        classify_path(&second.database()).unwrap(),
        SchemaClass::LegacyV4
    );
}

#[cfg(windows)]
#[test]
fn v4_history_converts_and_retains_the_verified_backup_pair() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-a", true);
    let connection = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
    let item: (String, i64) = connection
        .query_row("SELECT id, ordinal FROM items", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(item, ("event-1".into(), 1));
    drop(connection);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    assert!(paths.backup_final.exists());
    assert!(paths.manifest_final.exists());
    assert!(!paths.replacement_stage.exists());
    assert!(!paths.replacement_manifest.exists());
    assert!(!paths.retired_live.exists());
}

#[cfg(windows)]
#[test]
fn migrated_simulator_chat_lazily_creates_compatible_head_on_first_post_restart_prompt() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-a", true);

    let database = Database::open(&directory.database()).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    assert_eq!(initial.active_tab_id.as_deref(), Some("chat-a"));
    assert_eq!(initial.session_id, None);
    assert_eq!(initial.messages.len(), 1);
    assert_eq!(initial.messages[0].id, "event-1");
    drop(database);

    let reopened = Database::open(&directory.database()).unwrap();
    let appended = reopened
        .append_simulator_turn("chat-a", "continue", "ack")
        .unwrap();
    assert_ne!(appended.session_id, "session-a");
    assert_eq!(
        appended
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .next(),
        Some("event-1")
    );
    assert_eq!(appended.messages.len(), 3);
    let first_new_session = appended.session_id.clone();
    let legacy_shape: (String, String, i64, i64) = reopened
        .connection()
        .query_row(
            "SELECT state,residency,legacy,ordinal FROM agent_sessions WHERE id='session-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(legacy_shape, ("closed".into(), "cold".into(), 1, 0));
    let current_shape: (String, String, i64, i64) = reopened
        .connection()
        .query_row(
            "SELECT state,residency,legacy,ordinal FROM agent_sessions WHERE id=?1",
            [&first_new_session],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(current_shape, ("ready".into(), "cold".into(), 0, 1));

    let second = reopened
        .append_simulator_turn("chat-a", "continue again", "ack")
        .unwrap();
    assert_eq!(second.session_id, first_new_session);
    let session_count: i64 = reopened
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(session_count, 2);
    drop(reopened);

    let restarted = Database::open(&directory.database()).unwrap();
    let restored = restarted.bootstrap_snapshot().unwrap();
    assert_eq!(
        restored.session_id.as_deref(),
        Some(first_new_session.as_str())
    );
    assert_eq!(restored.messages.len(), 5);
    assert_eq!(restored.messages[0].id, "event-1");
}

#[cfg(windows)]
#[test]
fn lazy_simulator_attempt_and_first_turn_roll_back_together() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-a", true);
    let database = Database::open(&directory.database()).unwrap();
    database.bootstrap_snapshot().unwrap();
    let before = simulator_durable_state(&database, "project-a", "chat-a");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        database.append_simulator_turn_with_hook("chat-a", "continue", "ack", || {
            panic!("injected interruption")
        })
    }));
    assert!(result.is_err());
    assert_eq!(
        simulator_durable_state(&database, "project-a", "chat-a"),
        before
    );
    drop(database);

    let reopened = Database::open(&directory.database()).unwrap();
    assert_eq!(
        simulator_durable_state(&reopened, "project-a", "chat-a"),
        before
    );
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(restored.session_id, None);
    assert_eq!(restored.messages.len(), 1);
    assert_eq!(restored.messages[0].id, "event-1");
}

#[cfg(windows)]
#[test]
fn migrated_empty_chat_creates_its_first_simulator_attempt_without_a_session_anchor() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-a", false);
    let database = Database::open(&directory.database()).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    assert_eq!(initial.active_tab_id.as_deref(), Some("chat-a"));
    assert_eq!(initial.session_id, None);
    assert!(initial.messages.is_empty());
    drop(database);

    let reopened = Database::open(&directory.database()).unwrap();
    let appended = reopened
        .append_simulator_turn("chat-a", "first prompt", "ack")
        .unwrap();
    assert_eq!(appended.messages.len(), 2);
    assert!(
        appended
            .messages
            .iter()
            .all(|message| message.session_id == appended.session_id)
    );
    let session_shape: (i64, String, String, i64) = reopened
        .connection()
        .query_row(
            "SELECT ordinal,state,residency,legacy FROM agent_sessions WHERE id=?1",
            [&appended.session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(session_shape, (0, "ready".into(), "cold".into(), 0));
    drop(reopened);

    let reopened = Database::open(&directory.database()).unwrap();
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(restored.session_id, Some(appended.session_id));
    assert_eq!(restored.messages.len(), 2);
}

#[cfg(windows)]
fn simulator_durable_state(database: &Database, project_id: &str, chat_id: &str) -> Vec<i64> {
    database
        .connection()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM agent_sessions WHERE chat_id=?2),
               (SELECT COUNT(*) FROM chat_session_heads WHERE chat_id=?2),
               (SELECT COUNT(*) FROM turns WHERE chat_id=?2),
               (SELECT COUNT(*) FROM items WHERE chat_id=?2),
               (SELECT COUNT(*) FROM session_facts WHERE chat_id=?2),
               (SELECT COUNT(*) FROM session_runtime_epochs WHERE chat_id=?2),
               (SELECT projection_revision FROM chats WHERE id=?2),
               (SELECT updated_at FROM chats WHERE id=?2),
               (SELECT session_count FROM acp_project_usage WHERE project_id=?1),
               (SELECT turn_count FROM acp_project_usage WHERE project_id=?1),
               (SELECT item_count FROM acp_project_usage WHERE project_id=?1),
               (SELECT fact_count FROM acp_project_usage WHERE project_id=?1),
               (SELECT payload_bytes FROM acp_project_usage WHERE project_id=?1),
               (SELECT revision FROM acp_project_usage WHERE project_id=?1),
               (SELECT session_count FROM acp_store_usage WHERE singleton=1),
               (SELECT turn_count FROM acp_store_usage WHERE singleton=1),
               (SELECT item_count FROM acp_store_usage WHERE singleton=1),
               (SELECT fact_count FROM acp_store_usage WHERE singleton=1),
               (SELECT item_bytes FROM acp_store_usage WHERE singleton=1),
               (SELECT fact_bytes FROM acp_store_usage WHERE singleton=1),
               (SELECT shared_payload_bytes FROM acp_store_usage WHERE singleton=1),
               (SELECT revision FROM acp_store_usage WHERE singleton=1),
               (SELECT COUNT(*) FROM acp_session_usage WHERE chat_id=?2),
               COALESCE((SELECT SUM(turn_count) FROM acp_session_usage WHERE chat_id=?2),0),
               COALESCE((SELECT SUM(item_count) FROM acp_session_usage WHERE chat_id=?2),0),
               COALESCE((SELECT SUM(item_bytes) FROM acp_session_usage WHERE chat_id=?2),0),
               COALESCE((SELECT SUM(fact_count) FROM acp_session_usage WHERE chat_id=?2),0),
               COALESCE((SELECT SUM(fact_bytes) FROM acp_session_usage WHERE chat_id=?2),0),
               COALESCE((SELECT SUM(payload_bytes) FROM acp_session_usage WHERE chat_id=?2),0),
               COALESCE((SELECT SUM(revision) FROM acp_session_usage WHERE chat_id=?2),0)",
            params![project_id, chat_id],
            |row| (0..30).map(|index| row.get(index)).collect(),
        )
        .unwrap()
}

#[test]
fn session_limit_plus_one_fails_before_backup_artifact_creation() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-a", false);
    let connection = Connection::open(directory.database()).unwrap();
    for ordinal in 0..=4_096 {
        connection
            .execute(
                "INSERT INTO agent_sessions(
                   id, project_id, chat_tab_id, harness_id, status,
                   last_committed_sequence, created_at, updated_at
                 ) VALUES (?1,'project-a','chat-a','simulator','complete',0,?2,?2)",
                params![format!("session-{ordinal}"), ordinal],
            )
            .unwrap();
    }
    drop(connection);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!paths.backup_stage.exists());
    assert!(!paths.backup_final.exists());
    assert_eq!(
        classify_path(&directory.database()).unwrap(),
        SchemaClass::LegacyV4
    );
}

#[cfg(windows)]
#[test]
fn foreign_unmanifested_v5_stage_is_preserved_and_blocks() {
    let source = TestDir::new();
    let foreign = TestDir::new();
    create_v4(&source.database(), "source", false);
    create_v4(&foreign.database(), "foreign", true);
    drop(open_or_migrate(&foreign.database()).unwrap());
    let source_paths = MigrationPaths::new(&source.database()).unwrap();
    fs::copy(foreign.database(), &source_paths.replacement_stage).unwrap();
    let stage_before = file_fingerprint(&source_paths.replacement_stage).unwrap();
    assert!(open_or_migrate(&source.database()).is_err());
    assert_eq!(
        stage_before,
        file_fingerprint(&source_paths.replacement_stage).unwrap()
    );
    assert_eq!(
        classify_path(&source.database()).unwrap(),
        SchemaClass::LegacyV4
    );
}

#[test]
fn oversized_artifact_is_refused_before_hashing() {
    let directory = TestDir::new();
    let artifact = directory.0.join("sage.v5.db.stage");
    let file = fs::File::create(&artifact).unwrap();
    file.set_len(2).unwrap();
    drop(file);
    assert!(matches!(
        file_artifact::file_fingerprint_for_test(&artifact, 1),
        Err(AppError::ResourceLimit(_))
    ));
}

#[cfg(windows)]
#[test]
fn restart_adopts_the_complete_retirement_prefix() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "restart-retired", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let manifest = prepare_replacement(&paths);
    retire_manifested_source(&paths, &manifest);
    assert!(!paths.live.exists());
    let connection = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
    drop(connection);
    assert!(!paths.retired_live.exists());
    assert!(!paths.replacement_stage.exists());
    assert!(!paths.replacement_manifest.exists());
}

#[cfg(windows)]
#[test]
fn restart_cleans_a_published_v5_prefix_idempotently() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "restart-published", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let manifest = prepare_replacement(&paths);
    retire_manifested_source(&paths, &manifest);
    platform::rename_synced(&paths.replacement_stage, &paths.live).unwrap();
    let connection = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
    drop(connection);
    assert!(!paths.retired_live.exists());
    assert!(!paths.replacement_manifest.exists());
}

#[cfg(windows)]
#[test]
fn current_v5_open_rejects_a_tampered_retained_backup_without_touching_live() {
    use std::io::Write;
    let directory = TestDir::new();
    create_v4(&directory.database(), "tampered-backup", true);
    drop(open_or_migrate(&directory.database()).unwrap());
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let live_before = file_fingerprint(&paths.live).unwrap();
    let mut backup = fs::OpenOptions::new()
        .append(true)
        .open(&paths.backup_final)
        .unwrap();
    backup.write_all(b"tampered").unwrap();
    backup.sync_all().unwrap();
    drop(backup);
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(live_before, file_fingerprint(&paths.live).unwrap());
}

#[cfg(windows)]
#[test]
fn replacement_restore_prefixes_converge_sidecars_before_main() {
    for prefix in ["none", "shm", "wal", "main"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), prefix, true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let manifest = prepare_replacement(&paths);
        retire_manifested_source(&paths, &manifest);
        fs::remove_file(&paths.replacement_stage).unwrap();
        if matches!(prefix, "shm" | "wal" | "main") && manifest.source_after_checkpoint.shm.present
        {
            platform::rename_synced(&paths.retired_shm, &paths.live_shm).unwrap();
        }
        if matches!(prefix, "wal" | "main") && manifest.source_after_checkpoint.wal.present {
            platform::rename_synced(&paths.retired_wal, &paths.live_wal).unwrap();
        }
        if prefix == "main" {
            platform::rename_synced(&paths.retired_live, &paths.live).unwrap();
        }
        let connection = open_or_migrate(&directory.database()).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV22,
            "restore prefix {prefix}"
        );
        assert!(!paths.retired_live.exists());
        assert!(!paths.replacement_manifest.exists());
    }
}

#[cfg(windows)]
#[test]
fn valid_replacement_intent_does_not_delete_an_unowned_foreign_stage() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "foreign-stage", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        let backup = ensure(&paths).unwrap();
        build_intent::begin(&paths, &backup).unwrap();
        fs::write(&paths.replacement_stage, b"foreign-nonempty-stage").unwrap();
    }
    let before = fs::read(&paths.replacement_stage).unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(before, fs::read(&paths.replacement_stage).unwrap());
}

#[cfg(windows)]
#[test]
fn valid_replacement_intent_preserves_an_unowned_empty_stage() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "foreign-empty-stage", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        let backup = ensure(&paths).unwrap();
        build_intent::begin(&paths, &backup).unwrap();
        fs::File::create(&paths.replacement_stage).unwrap();
    }
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(paths.replacement_stage.exists());
    assert_eq!(fs::metadata(&paths.replacement_stage).unwrap().len(), 0);
}

#[cfg(windows)]
#[test]
fn replaced_owned_journal_is_preserved_and_blocks_recovery() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "replaced-journal", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        let backup = ensure(&paths).unwrap();
        build_intent::begin(&paths, &backup).unwrap();
        let intent = build_intent::read_intent(&paths).unwrap();
        let anchors = stage_owner::create_bound_stage(
            &paths.replacement_stage,
            &paths.replacement_journal,
            &paths.replacement_stage_owner,
            &intent.nonce,
        )
        .unwrap();
        drop(anchors);
        fs::remove_file(&paths.replacement_journal).unwrap();
        fs::write(&paths.replacement_journal, b"foreign-journal").unwrap();
    }
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(
        fs::read(&paths.replacement_journal).unwrap(),
        b"foreign-journal"
    );
    assert!(paths.replacement_stage.exists());
}

#[cfg(windows)]
#[test]
fn real_partial_sqlite_backup_copy_converges_after_restart() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "mid-copy", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let payload = "x".repeat(4_194_304);
    Connection::open(&paths.live)
        .unwrap()
        .execute(
            "INSERT INTO editor_buffers(
               project_id,root_id,relative_path,base_fingerprint,base_byte_len,
               base_modified_at,line_ending,buffer_content,buffer_byte_len,
               buffer_fingerprint,revision,pending_temp_path,updated_at
             ) VALUES ('project-a','root-a','large.txt',?1,0,NULL,'none',?2,?3,?1,1,NULL,1)",
            params!["a".repeat(64), payload, 4_194_304],
        )
        .unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        let backup = ensure(&paths).unwrap();
        build_intent::begin(&paths, &backup).unwrap();
        let intent = build_intent::read_intent(&paths).unwrap();
        let anchors = stage_owner::create_bound_stage(
            &paths.replacement_stage,
            &paths.replacement_journal,
            &paths.replacement_stage_owner,
            &intent.nonce,
        )
        .unwrap();
        let source = open_read_only(&paths.backup_final).unwrap();
        let mut destination = Connection::open(&paths.replacement_stage).unwrap();
        destination
            .pragma_update(None, "journal_mode", "TRUNCATE")
            .unwrap();
        destination.pragma_update(None, "cache_size", 1).unwrap();
        let backup_copy = rusqlite::backup::Backup::new(&source, &mut destination).unwrap();
        assert!(matches!(
            backup_copy.step(64).unwrap(),
            rusqlite::backup::StepResult::More
        ));
        let partial_stage = fs::read(&paths.replacement_stage).unwrap();
        let partial_journal = fs::read(&paths.replacement_journal).unwrap();
        assert!(!partial_stage.is_empty() || !partial_journal.is_empty());
        drop(backup_copy);
        drop(destination);
        drop(source);
        fs::write(&paths.replacement_stage, partial_stage).unwrap();
        fs::write(&paths.replacement_journal, partial_journal).unwrap();
        drop(anchors);
    }
    let connection = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
    assert!(!paths.replacement_stage_owner.exists());
    assert!(!paths.replacement_journal.exists());
}

#[cfg(windows)]
#[test]
fn injected_replacement_write_failure_preserves_legacy_and_retry_converges() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "injected-stage-failure", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    inject_replacement_build_failure_once();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(paths.backup_final.exists());
    assert!(paths.manifest_final.exists());
    assert!(paths.replacement_build_intent.exists());
    assert!(paths.replacement_stage_owner.exists());
    assert_eq!(
        fingerprint::classify_path(&paths.live).unwrap(),
        SchemaClass::LegacyV4
    );
    let connection = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
    assert!(!paths.replacement_build_intent.exists());
    assert!(!paths.replacement_stage_owner.exists());
}

#[cfg(feature = "acp-fixture")]
#[cfg(windows)]
pub(crate) struct FixtureBackupFailpointObservation {
    pub(crate) old_schema_readable: bool,
    pub(crate) new_schema_visible: bool,
    pub(crate) verified_retained_backup: bool,
    pub(crate) partial_rows: u64,
}

#[cfg(feature = "acp-fixture")]
#[cfg(windows)]
pub(crate) fn fixture_backup_failpoint_observation() -> FixtureBackupFailpointObservation {
    let directory = TestDir::new();
    create_v4(&directory.database(), "fixture-backup-failpoint", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    test_failpoint::arm("replacement-build");
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(
        !test_failpoint::is_armed(),
        "replacement-build was not reached"
    );
    let partial_rows = if paths.replacement_stage.exists() {
        fs::metadata(&paths.replacement_stage).unwrap().len()
    } else {
        0
    };
    let old_schema_readable = matches!(
        fingerprint::classify_path(&paths.backup_final),
        Ok(SchemaClass::LegacyV4)
    );
    let verified_retained_backup = backup::verify_retained_pair(&paths).is_ok();
    let connection = open_or_migrate(&directory.database()).unwrap();
    let new_schema_visible =
        fingerprint::classify_connection(&connection).unwrap() == SchemaClass::CurrentV22;
    FixtureBackupFailpointObservation {
        old_schema_readable,
        new_schema_visible,
        verified_retained_backup,
        partial_rows,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FixtureSqliteState {
    main: fingerprint::FileFingerprint,
    wal: fingerprint::FileFingerprint,
    shm: fingerprint::FileFingerprint,
    database: fingerprint::DatabaseFingerprint,
}

fn fixture_sqlite_state(paths: &MigrationPaths) -> FixtureSqliteState {
    let connection = open_read_only(&paths.live).unwrap();
    let database = database_fingerprint(&connection).unwrap();
    drop(connection);
    FixtureSqliteState {
        main: file_fingerprint(&paths.live).unwrap(),
        wal: file_fingerprint(&paths.live_wal).unwrap(),
        shm: file_fingerprint(&paths.live_shm).unwrap(),
        database,
    }
}

pub(crate) struct FixtureInsufficientSpaceObservation {
    pub(crate) accepted: bool,
    pub(crate) error: Option<&'static str>,
    pub(crate) sqlite_writes: u64,
    pub(crate) backup_created: bool,
}

pub(crate) fn fixture_insufficient_space_observation(
    available_bytes: u64,
) -> FixtureInsufficientSpaceObservation {
    let directory = TestDir::new();
    create_v4(
        &directory.database(),
        "fixture-migration-insufficient-space",
        true,
    );
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let before = fixture_sqlite_state(&paths);
    assert_eq!(before.database.schema, SchemaClass::LegacyV4);

    let available_bytes_guard = test_failpoint::override_available_bytes(available_bytes);
    let result = open_or_migrate(&directory.database());
    drop(available_bytes_guard);
    let (accepted, error) = match result {
        Ok(connection) => {
            drop(connection);
            (true, None)
        }
        Err(AppError::ResourceLimit(_)) => (false, Some("ResourceLimit")),
        Err(error) => panic!("expected typed ResourceLimit, received {error}"),
    };

    let after = fixture_sqlite_state(&paths);
    let sqlite_writes = if before == after { 0 } else { 1 };
    let backup_created = [
        &paths.backup_stage,
        &paths.backup_journal,
        &paths.backup_build_intent,
        &paths.backup_stage_owner,
        &paths.backup_final,
        &paths.manifest_stage,
        &paths.manifest_final,
    ]
    .iter()
    .any(|path| path.exists());

    FixtureInsufficientSpaceObservation {
        accepted,
        error,
        sqlite_writes,
        backup_created,
    }
}

#[test]
fn insufficient_space_preflight_preserves_v4_sqlite_and_creates_no_backup() {
    let observed = fixture_insufficient_space_observation(limits::MIGRATION_FREE_BYTES - 1);
    assert!(!observed.accepted);
    assert_eq!(observed.error, Some("ResourceLimit"));
    assert_eq!(observed.sqlite_writes, 0);
    assert!(!observed.backup_created);
}

#[cfg(feature = "acp-fixture")]
#[cfg(windows)]
pub(crate) struct FixtureRetirementObservation {
    pub(crate) verified_source_or_v5: bool,
    pub(crate) permanent_blocker_for_valid_prefix: bool,
    pub(crate) partial_logical_database: bool,
    pub(crate) unlisted_prefix: bool,
}

#[cfg(feature = "acp-fixture")]
#[cfg(windows)]
pub(crate) const FIXTURE_RETIREMENT_FAILPOINTS: &[&str] = &[
    "replacement-manifest-fsync",
    "replacement-build-intent-remove-or-parent-fsync",
    "retire-wal-rename-or-parent-fsync",
    "retire-shm-rename-or-parent-fsync",
    "retire-live-rename-or-parent-fsync",
    "replacement-publish",
    "postcommit-reopen",
    "cleanup-retired-shm-remove-or-parent-fsync",
    "cleanup-retired-wal-remove-or-parent-fsync",
    "cleanup-retired-live-remove-or-parent-fsync",
    "cleanup-replacement-manifest-remove-or-parent-fsync",
];

#[cfg(feature = "acp-fixture")]
#[cfg(windows)]
pub(crate) fn fixture_retirement_observation() -> FixtureRetirementObservation {
    for failpoint in FIXTURE_RETIREMENT_FAILPOINTS {
        let directory = TestDir::new();
        create_v4(&directory.database(), failpoint, true);
        test_failpoint::arm(failpoint);
        assert!(open_or_migrate(&directory.database()).is_err());
        assert!(
            !test_failpoint::is_armed(),
            "failpoint {failpoint} was not reached"
        );
        let connection = open_or_migrate(&directory.database()).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV22,
            "restart did not converge after {failpoint}"
        );
        verify_current(&connection).unwrap();
    }
    FixtureRetirementObservation {
        verified_source_or_v5: true,
        permanent_blocker_for_valid_prefix: false,
        partial_logical_database: false,
        unlisted_prefix: false,
    }
}

#[test]
fn project_and_root_preflight_bounds_accept_exact_and_reject_plus_one() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "project-root-bounds", false);
    let connection = Connection::open(directory.database()).unwrap();
    let mut bounds = limits::PRODUCTION_BOUNDS;
    bounds.projects = 1;
    bounds.roots = 1;
    bounds.roots_per_project = 1;
    limits::preflight_with_bounds(&connection, &directory.database(), bounds).unwrap();
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES ('project-b','Project B',1,1,1)",
            [],
        )
        .unwrap();
    assert!(limits::preflight_with_bounds(&connection, &directory.database(), bounds).is_err());
    connection
        .execute("DELETE FROM projects WHERE id='project-b'", [])
        .unwrap();
    let root = directory.0.join("root-b");
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    connection
        .execute(
            "INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
             VALUES ('root-b','project-a',?1,?2,1)",
            params![
                root.to_string_lossy().into_owned(),
                crate::project_registry::canonical_comparison_key(&root)
            ],
        )
        .unwrap();
    let mut global = limits::PRODUCTION_BOUNDS;
    global.roots = 1;
    assert!(limits::preflight_with_bounds(&connection, &directory.database(), global).is_err());
    let mut per_project = limits::PRODUCTION_BOUNDS;
    per_project.roots_per_project = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), per_project).is_err()
    );
}

#[test]
fn tab_buffer_and_event_preflight_bounds_accept_exact_and_reject_plus_one() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "row-bounds", true);
    let connection = Connection::open(directory.database()).unwrap();
    let mut exact = limits::PRODUCTION_BOUNDS;
    exact.tabs = 1;
    exact.tabs_per_project = 1;
    exact.events = 1;
    exact.events_per_session = 1;
    limits::preflight_with_bounds(&connection, &directory.database(), exact).unwrap();

    connection
        .execute(
            "INSERT INTO workbench_tabs(
               id,project_id,kind,title,position,pinned,dirty,status,
               metadata_json,created_at,updated_at
             ) VALUES ('chat-b','project-a','chat','Chat B',1,0,0,NULL,'{}',1,1)",
            [],
        )
        .unwrap();
    let mut global_tabs = limits::PRODUCTION_BOUNDS;
    global_tabs.tabs = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), global_tabs).is_err()
    );
    let mut project_tabs = limits::PRODUCTION_BOUNDS;
    project_tabs.tabs_per_project = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), project_tabs).is_err()
    );
    connection
        .execute("DELETE FROM workbench_tabs WHERE id='chat-b'", [])
        .unwrap();

    for (path, revision) in [("one.txt", 1), ("two.txt", 2)] {
        connection
            .execute(
                "INSERT INTO editor_buffers(
                   project_id,root_id,relative_path,base_fingerprint,base_byte_len,
                   base_modified_at,line_ending,buffer_content,buffer_byte_len,
                   buffer_fingerprint,revision,pending_temp_path,updated_at
                 ) VALUES ('project-a','root-a',?1,?2,0,NULL,'none','',0,?2,?3,NULL,1)",
                params![path, "a".repeat(64), revision],
            )
            .unwrap();
        let mut buffer_bounds = limits::PRODUCTION_BOUNDS;
        buffer_bounds.buffers = revision;
        buffer_bounds.buffers_per_project = revision;
        limits::preflight_with_bounds(&connection, &directory.database(), buffer_bounds).unwrap();
    }
    let mut global_buffers = limits::PRODUCTION_BOUNDS;
    global_buffers.buffers = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), global_buffers).is_err()
    );
    let mut project_buffers = limits::PRODUCTION_BOUNDS;
    project_buffers.buffers_per_project = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), project_buffers).is_err()
    );

    connection
        .execute(
            "INSERT INTO session_events(id,session_id,sequence,kind,payload_json,created_at)
             VALUES ('event-2','session-a',2,'message','{}',3)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET last_committed_sequence=2 WHERE id='session-a'",
            [],
        )
        .unwrap();
    let mut global_events = limits::PRODUCTION_BOUNDS;
    global_events.events = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), global_events).is_err()
    );
    let mut session_events = limits::PRODUCTION_BOUNDS;
    session_events.events_per_session = 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), session_events).is_err()
    );
}

#[test]
fn aggregate_preflight_bounds_accept_exact_bytes_and_reject_plus_one() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "aggregate-bounds", true);
    let connection = Connection::open(directory.database()).unwrap();
    let totals = limits::preflight_with_bounds(
        &connection,
        &directory.database(),
        limits::PRODUCTION_BOUNDS,
    )
    .unwrap();
    let mut exact = limits::PRODUCTION_BOUNDS;
    exact.session_metadata_bytes = totals.session_metadata_bytes;
    exact.payload_bytes = totals.payload_bytes;
    limits::preflight_with_bounds(&connection, &directory.database(), exact).unwrap();
    let mut metadata_plus_one = exact;
    metadata_plus_one.session_metadata_bytes -= 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), metadata_plus_one)
            .is_err()
    );
    let mut payload_plus_one = exact;
    payload_plus_one.payload_bytes -= 1;
    assert!(
        limits::preflight_with_bounds(&connection, &directory.database(), payload_plus_one)
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn owned_partial_backup_stage_is_removed_and_rebuilt_after_restart() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "backup-partial", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        let anchor = platform::anchor_existing_sqlite(&paths.live, true).unwrap();
        let source = Connection::open(&paths.live).unwrap();
        source.execute_batch("BEGIN IMMEDIATE").unwrap();
        let source_set = backup::source_set(&paths, &source).unwrap();
        let intent = backup::begin_backup_build(&paths, &source_set).unwrap();
        let stage_anchor = stage_owner::create_bound_stage(
            &paths.backup_stage,
            &paths.backup_journal,
            &paths.backup_stage_owner,
            &intent.nonce,
        )
        .unwrap();
        fs::write(&paths.backup_stage, b"owned-partial-backup").unwrap();
        stage_anchor.verify_path(&paths.backup_stage).unwrap();
        source.execute_batch("ROLLBACK").unwrap();
        anchor.verify_path(&paths.live).unwrap();
    }
    let connection = open_or_migrate(&directory.database()).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV22
    );
    assert!(!paths.backup_build_intent.exists());
    assert!(!paths.backup_stage_owner.exists());
}

#[test]
fn v4_project_bytes_are_rejected_before_backup_creation() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "oversized-tab", false);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    Connection::open(&paths.live)
        .unwrap()
        .execute(
            "UPDATE workbench_tabs SET title=?1 WHERE id='chat-a'",
            ["x".repeat(1025)],
        )
        .unwrap();
    assert!(matches!(
        open_or_migrate(&directory.database()),
        Err(AppError::ResourceLimit(_))
    ));
    assert!(!paths.backup_build_intent.exists());
    assert!(!paths.backup_stage.exists());
    assert!(!paths.backup_final.exists());
}

#[test]
fn busy_legacy_source_fails_before_backup_creation() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "busy", false);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let blocker = Connection::open(&paths.live).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert!(!paths.backup_build_intent.exists());
    assert!(!paths.backup_stage.exists());
    blocker.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn impossible_free_space_requirement_fails_deterministically() {
    assert!(matches!(
        platform::validate_available_bytes(u64::MAX - 1, u64::MAX),
        Err(AppError::ResourceLimit(_))
    ));
}

#[cfg(windows)]
#[test]
fn read_only_legacy_source_fails_before_backup_creation() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "read-only", false);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let original_permissions = fs::metadata(&paths.live).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&paths.live, permissions).unwrap();
    let result = open_or_migrate(&directory.database());
    fs::set_permissions(&paths.live, original_permissions).unwrap();
    assert!(result.is_err());
    assert!(!paths.backup_build_intent.exists());
    assert!(!paths.backup_stage.exists());
}

#[cfg(windows)]
#[test]
fn corrupt_live_wal_is_rejected_without_source_rewrite() {
    use std::io::{Seek, SeekFrom, Write};

    let directory = TestDir::new();
    create_v4(&directory.database(), "corrupt-wal", false);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let writer = Connection::open(&paths.live).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute("UPDATE projects SET name='wal-row'", [])
        .unwrap();
    let mut wal = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&paths.live_wal)
        .unwrap();
    wal.seek(SeekFrom::Start(0)).unwrap();
    wal.write_all(b"CORRUPT!").unwrap();
    wal.sync_all().unwrap();
    drop(wal);
    let before = (
        file_fingerprint(&paths.live).unwrap(),
        file_fingerprint(&paths.live_wal).unwrap(),
    );
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(
        before,
        (
            file_fingerprint(&paths.live).unwrap(),
            file_fingerprint(&paths.live_wal).unwrap(),
        )
    );
    drop(writer);
}

#[cfg(windows)]
#[test]
fn sqlite_path_anchor_prevents_delete_or_rename_while_live() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "anchor", false);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    let anchor = platform::anchor_existing_sqlite(&paths.live, true).unwrap();
    assert!(fs::remove_file(&paths.live).is_err());
    assert!(fs::rename(&paths.live, directory.0.join("moved.db")).is_err());
    anchor.verify_path(&paths.live).unwrap();
    drop(anchor);
    assert!(paths.live.exists());
}

#[cfg(windows)]
#[test]
fn opened_database_keeps_its_live_anchor_until_the_guard_drops() {
    let directory = TestDir::new();
    let path = directory.database();
    let moved = directory.0.join("moved.db");
    let opened = open_or_migrate(&path).unwrap();
    assert!(fs::rename(&path, &moved).is_err());
    let (connection, anchor) = opened.into_parts();
    drop(connection);
    assert!(fs::rename(&path, &moved).is_err());
    drop(anchor);
    fs::rename(&path, &moved).unwrap();
}

#[cfg(windows)]
#[test]
fn anchor_open_reparse_swap_is_rejected_before_sqlite_touches_target() {
    let directory = TestDir::new();
    let original = directory.database();
    drop(open_or_migrate(&original).unwrap());
    let displaced = directory.0.join("displaced.db");
    let target = directory.0.join("target.db");
    let target_bytes = b"not-a-sqlite-target".to_vec();
    fs::write(&target, &target_bytes).unwrap();
    platform_anchor::inject_anchor_open_reparse(move |path| {
        fs::rename(path, &displaced).unwrap();
        fs::rename(&target, path).unwrap();
    });
    assert!(open_or_migrate(&original).is_err());
    assert_eq!(fs::read(&original).unwrap(), target_bytes);
}

#[cfg(windows)]
#[test]
fn native_stage_identity_rejects_replaced_file_without_deleting_it() {
    let directory = TestDir::new();
    create_v4(&directory.database(), "replaced-stage", true);
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    {
        let _lock = MigrationLock::acquire(paths.parent()).unwrap();
        let backup = ensure(&paths).unwrap();
        build_intent::begin(&paths, &backup).unwrap();
        let intent = build_intent::read_intent(&paths).unwrap();
        let anchor = stage_owner::create_bound_stage(
            &paths.replacement_stage,
            &paths.replacement_journal,
            &paths.replacement_stage_owner,
            &intent.nonce,
        )
        .unwrap();
        drop(anchor);
        fs::remove_file(&paths.replacement_stage).unwrap();
        fs::write(&paths.replacement_stage, b"foreign-replacement").unwrap();
    }
    let before = fs::read(&paths.replacement_stage).unwrap();
    assert!(open_or_migrate(&directory.database()).is_err());
    assert_eq!(before, fs::read(&paths.replacement_stage).unwrap());
}

#[cfg(not(windows))]
#[test]
fn identity_bound_sqlite_deletion_fails_closed_without_mutating_stage() {
    let directory = TestDir::new();
    let stage = directory.0.join("owned-stage.db");
    fs::write(&stage, b"owned-stage").unwrap();
    let anchor = platform_anchor::anchor_existing_sqlite(&stage, true).unwrap();
    let identity = anchor.identity().clone();
    drop(anchor);
    let before = fs::read(&stage).unwrap();

    let error = platform_anchor::delete_file_with_identity(&stage, &identity)
        .expect_err("identity-bound deletion must remain unsupported off Windows");

    assert!(
        matches!(error, crate::error::AppError::InvalidRequest(message) if message == "identity-bound SQLite deletion is not admitted on this platform")
    );
    assert_eq!(before, fs::read(&stage).unwrap());
}

#[cfg(unix)]
#[test]
fn file_backed_backup_creation_refuses_identity_deletion_and_preserves_source_on_retry() {
    for version in [4, 13, 16, 17] {
        let directory = TestDir::new();
        create_v4(&directory.database(), "unsupported-backup", true);
        let mut source = Connection::open(directory.database()).unwrap();
        match version {
            4 => {}
            13 => schema_upgrade::apply_through_v13(&mut source).unwrap(),
            16 => schema_upgrade::apply_through_v16(&mut source).unwrap(),
            17 => schema_upgrade::apply_through_v17(&mut source).unwrap(),
            _ => unreachable!(),
        }
        source.close().unwrap();
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let before = fixture_sqlite_state(&paths);
        let snapshot = || {
            let mut files: Vec<_> = fs::read_dir(&directory.0)
                .unwrap()
                .map(|entry| entry.unwrap())
                .filter(|entry| entry.file_type().unwrap().is_file())
                .map(|entry| (entry.file_name(), fs::read(entry.path()).unwrap()))
                .collect();
            files.sort_by(|left, right| left.0.cmp(&right.0));
            files
        };
        let mut retained = None;
        for _ in 0..2 {
            let error = open_or_migrate(&directory.database())
                .err()
                .expect("Unix cannot retire the identity-bound backup journal");
            assert!(
                matches!(error, AppError::InvalidRequest(message) if message == "identity-bound SQLite deletion is not admitted on this platform")
            );
            assert_eq!(fixture_sqlite_state(&paths), before, "source v{version}");
            let after = snapshot();
            if let Some(retained) = &retained {
                assert_eq!(&after, retained, "retry must preserve recovery artifacts");
            }
            retained = Some(after);
        }
    }
}

#[cfg(windows)]
#[test]
fn replacement_retirement_prefixes_converge() {
    for prefix in ["none", "wal", "shm", "main"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), prefix, true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let manifest = prepare_replacement(&paths);
        if matches!(prefix, "wal" | "shm" | "main") && manifest.source_after_checkpoint.wal.present
        {
            platform::rename_synced(&paths.live_wal, &paths.retired_wal).unwrap();
        }
        if matches!(prefix, "shm" | "main") && manifest.source_after_checkpoint.shm.present {
            platform::rename_synced(&paths.live_shm, &paths.retired_shm).unwrap();
        }
        if prefix == "main" {
            platform::rename_synced(&paths.live, &paths.retired_live).unwrap();
        }
        let connection = open_or_migrate(&directory.database()).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV22,
            "retirement prefix {prefix}"
        );
    }
}

#[cfg(windows)]
#[test]
fn replacement_cleanup_prefixes_converge() {
    for prefix in ["none", "shm", "wal", "main"] {
        let directory = TestDir::new();
        create_v4(&directory.database(), prefix, true);
        let paths = MigrationPaths::new(&directory.database()).unwrap();
        let manifest = prepare_replacement(&paths);
        retire_manifested_source(&paths, &manifest);
        platform::rename_synced(&paths.replacement_stage, &paths.live).unwrap();
        if matches!(prefix, "shm" | "wal" | "main") && paths.retired_shm.exists() {
            platform::remove_synced(&paths.retired_shm).unwrap();
        }
        if matches!(prefix, "wal" | "main") && paths.retired_wal.exists() {
            platform::remove_synced(&paths.retired_wal).unwrap();
        }
        if prefix == "main" {
            platform::remove_synced(&paths.retired_live).unwrap();
        }
        let connection = open_or_migrate(&directory.database()).unwrap();
        assert_eq!(
            fingerprint::classify_connection(&connection).unwrap(),
            SchemaClass::CurrentV22,
            "cleanup prefix {prefix}"
        );
        assert!(!paths.replacement_manifest.exists());
    }
}

#[cfg(unix)]
#[test]
fn migration_artifact_symlink_is_rejected_without_touching_target() {
    use std::os::unix::fs::symlink;
    let directory = TestDir::new();
    let target = directory.0.join("outside.db");
    fs::write(&target, b"do-not-touch").unwrap();
    symlink(&target, directory.database()).unwrap();
    let before = fs::read(&target).unwrap();
    assert!(MigrationPaths::new(&directory.database()).is_err());
    assert_eq!(before, fs::read(&target).unwrap());
}

#[cfg(windows)]
fn create_current_schema(path: &Path, schema: SchemaClass, project_name: &str) {
    create_v4(path, project_name, true);
    let mut connection = Connection::open(path).unwrap();
    apply_v5(&mut connection).unwrap();
    if schema == SchemaClass::CurrentV6 {
        apply_v6(&mut connection).unwrap();
    }
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        schema
    );
    connection.close().unwrap();
}

#[cfg(windows)]
fn assert_current_v5_remains_unchanged(path: &Path, project_name: &str) {
    let before = file_fingerprint(path).unwrap();

    assert!(open_or_migrate(path).is_err());

    assert_eq!(before, file_fingerprint(path).unwrap());
    let connection = open_read_only(path).unwrap();
    assert_eq!(
        fingerprint::classify_connection(&connection).unwrap(),
        SchemaClass::CurrentV5
    );
    let actual_name: String = connection
        .query_row(
            "SELECT name FROM projects WHERE id='project-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(actual_name, project_name);
}

fn create_v4(path: &Path, project_id: &str, event: bool) {
    let mut connection = Connection::open(path).unwrap();
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(V2).unwrap();
    connection.execute_batch(V3).unwrap();
    project_migration::migrate(&mut connection).unwrap();
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES (?1,?2,1,1,1)",
            params!["project-a", project_id],
        )
        .unwrap();
    let root = path.parent().unwrap().join("root");
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let root_text = root.to_string_lossy().into_owned();
    let root_key = crate::project_registry::canonical_comparison_key(&root);
    connection
        .execute(
            "INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
             VALUES ('root-a','project-a',?1,?2,0)",
            params![root_text, root_key],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workbench_tabs(
               id,project_id,kind,title,position,pinned,dirty,status,
               metadata_json,created_at,updated_at
             ) VALUES ('chat-a','project-a','chat','Chat',0,0,0,NULL,'{}',1,1)",
            [],
        )
        .unwrap();
    if event {
        connection
            .execute(
                "INSERT INTO agent_sessions(
                   id,project_id,chat_tab_id,harness_id,status,
                   last_committed_sequence,created_at,updated_at
                 ) VALUES ('session-a','project-a','chat-a','simulator','complete',1,1,2)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session_events(id,session_id,sequence,kind,payload_json,created_at)
                 VALUES ('event-1','session-a',1,'message','{\"role\":\"assistant\",\"text\":\"hello\"}',2)",
                [],
            )
            .unwrap();
    }
    connection.close().unwrap();
}

#[cfg(windows)]
fn prepare_replacement(paths: &MigrationPaths) -> ReplacementManifest {
    let _lock = MigrationLock::acquire(paths.parent()).unwrap();
    let backup = ensure(paths).unwrap();
    build_intent::begin(paths, &backup).unwrap();
    let intent = build_intent::read_intent(paths).unwrap();
    build_replacement(paths, &backup, &intent.nonce).unwrap();
    let source_after = checkpoint_source(paths, &backup).unwrap();
    let replacement = file_fingerprint(&paths.replacement_stage).unwrap();
    let connection = open_read_only(&paths.replacement_stage).unwrap();
    let replacement_database = database_fingerprint(&connection).unwrap();
    drop(connection);
    let manifest = ReplacementManifest::new(
        backup.source,
        source_after,
        backup.backup_file,
        replacement,
        replacement_database,
    );
    write_stage(&paths.replacement_manifest, &manifest).unwrap();
    build_intent::finalize_published_manifest(paths).unwrap();
    manifest
}

#[cfg(windows)]
fn retire_manifested_source(paths: &MigrationPaths, manifest: &ReplacementManifest) {
    let source = &manifest.source_after_checkpoint;
    if source.wal.present {
        platform::rename_synced(&paths.live_wal, &paths.retired_wal).unwrap();
    }
    if source.shm.present {
        platform::rename_synced(&paths.live_shm, &paths.retired_shm).unwrap();
    }
    platform::rename_synced(&paths.live, &paths.retired_live).unwrap();
}

fn create_wal_source(label: &str) -> (TestDir, Connection, MigrationPaths) {
    let directory = TestDir::new();
    create_v4(&directory.database(), label, false);
    let writer = Connection::open(directory.database()).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute(
            "UPDATE projects SET updated_at=updated_at+1 WHERE id='project-a'",
            [],
        )
        .unwrap();
    let paths = MigrationPaths::new(&directory.database()).unwrap();
    (directory, writer, paths)
}

fn assert_probe_clean(paths: &MigrationPaths) {
    assert!(!paths.probe_main.exists());
    assert!(!paths.probe_wal.exists());
    assert!(!paths.probe_shm.exists());
    assert!(!paths.probe_manifest.exists());
}

#[path = "populated_v8_tests.rs"]
mod populated_v8_tests;

#[path = "image_kind_tests.rs"]
mod image_kind_tests;

#[path = "v18_backup_tests.rs"]
mod v18_backup_tests;
