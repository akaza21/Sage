use crate::chat_archive::ChatArchiveStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::session_events::SessionEventStore;
use crate::workbench_persistence::store::WorkbenchStore;
use rusqlite::params;

use super::*;
use sage_domain::chat::{ChatLifecycleRequest, ProjectLayoutRequest};

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
    let root = std::env::temp_dir().join(format!("sage-data-cleanup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("a")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let project = database.create_project_with_chat(&root.join("a")).unwrap().id;
    Fixture {
        root,
        database,
        project,
    }
}

fn new_chat(database: &Database, project: &str) -> String {
    let mut connection = database.write_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    let chat = crate::chat_lifecycle::insert_new_chat(&transaction, project, true, None).unwrap();
    transaction.commit().unwrap();
    chat
}

fn archive(database: &Database, project: &str, chat: &str) {
    let snapshot = database.bootstrap_snapshot().unwrap();
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: project.into(),
            chat_id: chat.into(),
            current_layout: ProjectLayoutRequest {
                project_id: project.into(),
                tabs: snapshot.tabs.clone(),
                active_tab_id: snapshot.active_tab_id.clone(),
                selected_root_id: snapshot.selected_root_id.clone(),
                selected_resource: snapshot.selected_resource.clone(),
            },
        })
        .unwrap();
}

fn fork(database: &Database, project: &str, parent: &str) -> String {
    let child = new_chat(database, project);
    database
        .connection()
        .execute(
            "INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,
               parent_chat_id,source_session_id,source_turn_id,relation,created_at)
             SELECT ?1,?2,project_id,chat_id,session_id,id,'nativeFork',1
               FROM turns WHERE project_id=?1 AND chat_id=?3 ORDER BY ordinal LIMIT 1",
            params![project, child, parent],
        )
        .unwrap();
    child
}

fn count(database: &Database, sql: &str, value: &str) -> i64 {
    database
        .connection()
        .query_row(sql, [value], |row| row.get(0))
        .unwrap()
}

fn with_history(database: &Database, project: &str) -> (String, String) {
    let chat = new_chat(database, project);
    database
        .append_simulator_turn(&chat, "old prompt", "old answer")
        .unwrap();
    let image = sage_images::insert_test_image(
        &database.connection(),
        database.image_artifact_root().unwrap(),
        project,
        &chat,
        "old.png",
    )
    .artifact_id;
    (chat, image)
}

#[test]
fn deleting_archived_chats_purges_forks_first_and_keeps_attached_chats() {
    let fixture = fixture();
    let Fixture {
        database, project, ..
    } = &fixture;
    let attached = database.bootstrap_snapshot().unwrap().active_tab_id.unwrap();
    let (parent, image) = with_history(database, project);
    let child = fork(database, project, &parent);
    // The parent is archived first, so its fork sorts after it by nothing
    // but lineage.
    archive(database, project, &parent);
    archive(database, project, &child);

    let deleted = database.clean_up(project, CleanupScope::Archived).unwrap();

    assert_eq!(deleted, 2);
    for chat in [&parent, &child] {
        assert_eq!(count(database, "SELECT COUNT(*) FROM chats WHERE id=?1", chat), 0);
        assert_eq!(count(database, "SELECT COUNT(*) FROM turns WHERE chat_id=?1", chat), 0);
    }
    assert_eq!(count(database, "SELECT COUNT(*) FROM chats WHERE id=?1", &attached), 1);
    let artifacts = database.image_artifact_root().unwrap();
    assert!(!artifacts.join(format!("{image}.full")).exists());
    fixture.finish();
}

#[test]
fn removing_a_project_deletes_its_chats_roots_and_activity_only() {
    let fixture = fixture();
    let Fixture {
        root,
        database,
        project,
    } = &fixture;
    std::fs::create_dir_all(root.join("b")).unwrap();
    let other = database.create_project_with_chat(&root.join("b")).unwrap().id;
    let (parent, image) = with_history(database, project);
    fork(database, project, &parent);
    archive(database, project, &parent);
    database
        .connection()
        .execute("UPDATE app_state SET active_project_id=?1", [project])
        .unwrap();
    let other_chats = count(database, "SELECT COUNT(*) FROM chats WHERE project_id=?1", &other);

    let deleted = database.clean_up(project, CleanupScope::Project).unwrap();

    assert_eq!(deleted, 3);
    for table in ["projects WHERE id", "project_roots WHERE project_id", "chats WHERE project_id", "turns WHERE project_id", "acp_project_usage WHERE project_id"] {
        assert_eq!(count(database, &format!("SELECT COUNT(*) FROM {table}=?1"), project), 0, "{table}");
    }
    let active: Option<String> = database
        .connection()
        .query_row("SELECT active_project_id FROM app_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(active, None);
    assert_eq!(count(database, "SELECT COUNT(*) FROM projects WHERE id=?1", &other), 1);
    assert_eq!(count(database, "SELECT COUNT(*) FROM chats WHERE project_id=?1", &other), other_chats);
    assert!(root.join("a").is_dir(), "the Project's folder stays on disk");
    let artifacts = database.image_artifact_root().unwrap();
    assert!(!artifacts.join(format!("{image}.full")).exists());
    fixture.finish();
}

#[test]
fn removing_a_project_with_an_unsettled_runtime_changes_nothing() {
    let fixture = fixture();
    let Fixture {
        database, project, ..
    } = &fixture;
    let (chat, _) = with_history(database, project);
    database
        .connection()
        .execute(
            "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
             SELECT project_id,chat_id,id,100,'running',1 FROM agent_sessions WHERE chat_id=?1",
            [&chat],
        )
        .unwrap();
    let chats = count(database, "SELECT COUNT(*) FROM chats WHERE project_id=?1", project);

    let error = database.clean_up(project, CleanupScope::Project).unwrap_err();

    assert!(error.to_string().contains("shutting down"), "{error}");
    assert_eq!(count(database, "SELECT COUNT(*) FROM chats WHERE project_id=?1", project), chats);
    assert_eq!(count(database, "SELECT COUNT(*) FROM projects WHERE id=?1", project), 1);
    fixture.finish();
}
