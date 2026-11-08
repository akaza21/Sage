use rusqlite::params;

use super::*;
use crate::{
    acp::limits::LIMITS,
    domain::{BootstrapSnapshot, ChatLifecycleRequest, ProjectLayoutRequest},
};

struct Fixture {
    root: std::path::PathBuf,
    database: Database,
    project: String,
}

impl Fixture {
    fn finish(self) {
        drop(self.database);
        std::fs::remove_dir_all(self.root).unwrap();
    }
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("sage-chat-delete-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = Database::open(&root.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&root).unwrap().id;
    Fixture {
        root,
        database,
        project,
    }
}

fn layout(project: &str, snapshot: &BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: project.into(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

fn new_chat(database: &Database, project: &str) -> AppResult<String> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction()?;
    let chat = crate::chat_lifecycle::insert_new_chat(&transaction, project, true, None)?;
    transaction.commit()?;
    Ok(chat)
}

fn archive(database: &Database, project: &str, chat: &str) {
    let snapshot = database.bootstrap_snapshot().unwrap();
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: project.into(),
            chat_id: chat.into(),
            current_layout: layout(project, &snapshot),
        })
        .unwrap();
}

/// A Chat with history, a server request and an image, then archived.
fn archived_chat_with_history(fixture: &Fixture) -> (String, String) {
    let Fixture {
        database, project, ..
    } = fixture;
    let chat = new_chat(database, project).unwrap();
    database
        .append_simulator_turn(&chat, "first prompt", "first answer")
        .unwrap();
    database
        .append_simulator_turn(&chat, "second prompt", "second answer")
        .unwrap();
    {
        let connection = database.connection();
        connection
            .execute(
                "INSERT INTO server_requests(
                   id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
                   external_request_id,kind,payload_digest_version,payload_digest,state,
                   normalized_payload_json,created_at,updated_at
                 ) SELECT 'request-delete',t.project_id,t.chat_id,t.session_id,t.id,
                          t.runtime_epoch,s.binding_digest,'external-delete','permission',1,
                          ?3,'received','{}',1,1
                   FROM turns t JOIN agent_sessions s ON s.id=t.session_id
                   WHERE t.project_id=?1 AND t.chat_id=?2 ORDER BY t.ordinal LIMIT 1",
                params![project, chat, "a".repeat(64)],
            )
            .unwrap();
    }
    let image = crate::image_artifacts::insert_test_image(
        &database.connection(),
        database.image_artifact_root().unwrap(),
        project,
        &chat,
        "deleted.png",
    )
    .artifact_id;
    archive(database, project, &chat);
    (chat, image)
}

type Usage = (Vec<Vec<i64>>, Vec<i64>);

fn usage(connection: &rusqlite::Connection) -> Usage {
    let projects = connection
        .prepare(
            "SELECT chat_count,session_count,turn_count,item_count,fact_count,payload_bytes
             FROM acp_project_usage ORDER BY project_id",
        )
        .unwrap()
        .query_map([], |row| (0..6).map(|index| row.get(index)).collect())
        .unwrap()
        .collect::<Result<Vec<Vec<i64>>, _>>()
        .unwrap();
    let store = connection
        .query_row(
            "SELECT chat_count,session_count,turn_count,item_count,item_bytes,fact_count,
                    fact_bytes,shared_payload_bytes
             FROM acp_store_usage WHERE singleton=1",
            [],
            |row| (0..8).map(|index| row.get(index)).collect(),
        )
        .unwrap();
    (projects, store)
}

/// The stored usage ledger; the connection lock is released on return.
fn ledger(database: &Database) -> Usage {
    usage(&database.connection())
}

/// The usage ledger rebuilt from the remaining rows by the migration's own
/// recount; the rebuild is rolled back.
fn recount(database: &Database) -> Usage {
    let mut connection = database.connection();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute_batch(
            "DELETE FROM acp_session_usage;
             DELETE FROM acp_project_usage;
             DELETE FROM acp_store_usage;",
        )
        .unwrap();
    transaction
        .execute_batch(include_str!("../acp/migration/ledger.sql"))
        .unwrap();
    usage(&transaction)
}

fn count(database: &Database, sql: &str, chat: &str) -> i64 {
    database
        .connection()
        .query_row(sql, [chat], |row| row.get(0))
        .unwrap()
}

/// Records a workspace change for the Chat's session under an intent no turn
/// carries, charged the way the runtime charges it.
fn remember_orphan_workspace_change(fixture: &Fixture, chat: &str) -> bool {
    let session: String = fixture
        .database
        .connection()
        .query_row(
            "SELECT id FROM agent_sessions WHERE chat_id=?1 LIMIT 1",
            [chat],
            |row| row.get(0),
        )
        .unwrap();
    let mut connection = fixture.database.write_connection().unwrap();
    crate::turn_workspace::remember(
        &mut connection,
        &session,
        "orphan-intent",
        "root",
        &fixture.root,
        None,
    )
    .unwrap()
}

fn guards_intact(database: &Database) -> bool {
    let present: i64 = database
        .connection()
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name IN ({})",
                crate::chat_purge::HISTORY_GUARDS
                    .iter()
                    .map(|name| format!("'{name}'"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            [],
            |row| row.get(0),
        )
        .unwrap();
    present == crate::chat_purge::HISTORY_GUARDS.len() as i64
}

fn request(project: &str, chat: &str) -> ChatDeleteRequest {
    ChatDeleteRequest {
        project_id: project.into(),
        chat_id: chat.into(),
    }
}

#[test]
fn deleting_an_archived_chat_frees_a_slot_at_the_project_chat_limit() {
    let fixture = fixture();
    let (archived, _) = archived_chat_with_history(&fixture);
    let Fixture {
        database, project, ..
    } = &fixture;
    let limit = LIMITS.durable.chats_per_project as i64;
    let existing = count(
        database,
        "SELECT COUNT(*) FROM chats WHERE project_id=?1",
        project,
    );
    for _ in existing..limit {
        new_chat(database, project).unwrap();
    }
    assert!(matches!(
        new_chat(database, project),
        Err(AppError::ResourceLimit(_))
    ));

    database
        .delete_archived_chat(&request(project, &archived))
        .unwrap();

    new_chat(database, project).unwrap();
    assert_eq!(
        count(
            database,
            "SELECT COUNT(*) FROM chats WHERE project_id=?1",
            project
        ),
        limit
    );
    assert_eq!(ledger(database), recount(database));
    fixture.finish();
}

#[test]
fn deleting_an_archived_chat_reclaims_every_row_file_and_usage_counter() {
    let fixture = fixture();
    let Fixture {
        database, project, ..
    } = &fixture;
    // A surviving Chat with history keeps its own charge.
    let kept = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    database
        .append_simulator_turn(&kept, "kept prompt", "kept answer")
        .unwrap();
    let (archived, image) = archived_chat_with_history(&fixture);
    assert_eq!(ledger(database), recount(database));
    // A workspace change no turn owns still charges its session; the purge
    // must release it once, not again through the session cascade.
    assert!(remember_orphan_workspace_change(&fixture, &archived));
    let before = ledger(database);
    assert!(before.1[7] > recount(database).1[7], "orphan is charged");

    database
        .delete_archived_chat(&request(project, &archived))
        .unwrap();

    let after = ledger(database);
    assert_eq!(after, recount(database));
    assert_eq!(after.1[0], before.1[0] - 1, "store chat count");
    assert!(after.1[2] < before.1[2], "store turn count");
    for table in [
        "chats WHERE id=?1",
        "agent_sessions WHERE chat_id=?1",
        "turns WHERE chat_id=?1",
        "items WHERE chat_id=?1",
        "session_facts WHERE chat_id=?1",
        "server_requests WHERE chat_id=?1",
        "chat_image_artifacts WHERE chat_id=?1",
        "acp_turn_workspace_changes WHERE ?1 IS NOT NULL",
    ] {
        assert_eq!(
            count(
                database,
                &format!("SELECT COUNT(*) FROM {table}"),
                &archived
            ),
            0,
            "{table}"
        );
    }
    let artifacts = database.image_artifact_root().unwrap();
    assert!(!artifacts.join(format!("{image}.full")).exists());
    assert!(!artifacts.join(format!("{image}.thumb")).exists());
    assert!(
        database
            .bootstrap_snapshot()
            .unwrap()
            .archived_chats
            .is_empty()
    );
    fixture.finish();
}

#[test]
fn deleting_an_attached_or_running_chat_is_refused_and_changes_nothing() {
    let fixture = fixture();
    let (archived, image) = archived_chat_with_history(&fixture);
    let Fixture {
        database, project, ..
    } = &fixture;
    let attached = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    database
        .append_simulator_turn(&attached, "attached prompt", "attached answer")
        .unwrap();
    let before = ledger(database);

    let error = database
        .delete_archived_chat(&request(project, &attached))
        .unwrap_err();
    assert!(error.to_string().contains("archived"), "{error}");

    database
        .connection()
        .execute(
            "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
             SELECT project_id,chat_id,id,100,'running',1 FROM agent_sessions WHERE chat_id=?1",
            [&archived],
        )
        .unwrap();
    let error = database
        .delete_archived_chat(&request(project, &archived))
        .unwrap_err();
    assert!(error.to_string().contains("Stop"), "{error}");

    assert!(guards_intact(database));
    assert_eq!(ledger(database), before);
    for chat in [&attached, &archived] {
        assert_eq!(
            count(database, "SELECT COUNT(*) FROM chats WHERE id=?1", chat),
            1
        );
        assert!(
            count(
                database,
                "SELECT COUNT(*) FROM turns WHERE chat_id=?1",
                chat
            ) > 0
        );
    }
    let artifacts = database.image_artifact_root().unwrap();
    assert!(artifacts.join(format!("{image}.full")).is_file());
    fixture.finish();
}

#[test]
fn deleting_a_chat_that_forks_descend_from_is_refused() {
    let fixture = fixture();
    let (archived, image) = archived_chat_with_history(&fixture);
    let Fixture {
        database, project, ..
    } = &fixture;
    let fork = new_chat(database, project).unwrap();
    database
        .connection()
        .execute(
            "INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,
               parent_chat_id,source_session_id,source_turn_id,relation,created_at)
             SELECT ?1,?2,project_id,chat_id,session_id,id,'nativeFork',1
               FROM turns WHERE project_id=?1 AND chat_id=?3 ORDER BY ordinal LIMIT 1",
            params![project, fork, archived],
        )
        .unwrap();
    let before = ledger(database);

    let error = database
        .delete_archived_chat(&request(project, &archived))
        .unwrap_err();
    assert!(
        matches!(&error, AppError::InvalidRequest(message)
            if message == "Delete the Chats forked from this Chat first"),
        "{error:?}"
    );

    assert!(guards_intact(database));
    assert_eq!(ledger(database), before);
    assert_eq!(
        count(
            database,
            "SELECT COUNT(*) FROM chats WHERE id=?1",
            &archived
        ),
        1
    );
    let artifacts = database.image_artifact_root().unwrap();
    assert!(artifacts.join(format!("{image}.full")).is_file());
    fixture.finish();
}

/// Everything the purge did, image rows included, rolls back when it fails
/// after the guards are lifted; image files are untouched until COMMIT.
fn assert_purge_rolled_back(fixture: &Fixture, chat: &str, image: &str, before: &Usage) {
    let database = &fixture.database;
    assert!(guards_intact(database));
    assert_eq!(&ledger(database), before);
    for table in [
        "chats WHERE id=?1",
        "turns WHERE chat_id=?1",
        "chat_image_artifacts WHERE chat_id=?1",
    ] {
        assert!(
            count(database, &format!("SELECT COUNT(*) FROM {table}"), chat) > 0,
            "{table}"
        );
    }
    let artifacts = database.image_artifact_root().unwrap();
    assert!(artifacts.join(format!("{image}.full")).is_file());
    assert!(artifacts.join(format!("{image}.thumb")).is_file());
}

#[test]
fn a_delete_refused_at_commit_keeps_the_chat_its_images_and_every_guard() {
    let fixture = fixture();
    let (archived, image) = archived_chat_with_history(&fixture);
    let Fixture {
        database, project, ..
    } = &fixture;
    // A deferrable (NO ACTION) reference the purge does not delete.
    database
        .connection()
        .execute_batch(
            "CREATE TABLE purge_probe(project_id TEXT NOT NULL, chat_id TEXT NOT NULL,
               FOREIGN KEY (project_id, chat_id) REFERENCES chats(project_id, id))",
        )
        .unwrap();
    database
        .connection()
        .execute(
            "INSERT INTO purge_probe VALUES (?1, ?2)",
            [project, &archived],
        )
        .unwrap();
    let before = ledger(database);

    let error = database
        .delete_archived_chat(&request(project, &archived))
        .unwrap_err();
    assert!(
        matches!(
            &error,
            AppError::Database(rusqlite::Error::SqliteFailure(failure, _))
                if failure.extended_code == 787
        ),
        "refused at COMMIT by the deferred foreign-key check: {error:?}"
    );
    assert_purge_rolled_back(&fixture, &archived, &image, &before);
    fixture.finish();
}

#[test]
fn a_purge_refused_by_admission_inside_its_transaction_changes_nothing() {
    let fixture = fixture();
    let (archived, image) = archived_chat_with_history(&fixture);
    let Fixture {
        database, project, ..
    } = &fixture;
    let before = ledger(database);
    // Admitted before the transaction, then no longer qualifying inside it.
    let calls = std::cell::Cell::new(0);
    let error = database
        .purge_chat(project, &archived, |_| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Ok(())
            } else {
                Err(AppError::InvalidRequest("stopped qualifying".into()))
            }
        })
        .unwrap_err();
    assert!(error.to_string().contains("stopped qualifying"), "{error}");
    assert_eq!(calls.get(), 2);
    assert_purge_rolled_back(&fixture, &archived, &image, &before);
    fixture.finish();
}
