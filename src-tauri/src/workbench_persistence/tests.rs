use std::{fs, path::Path};

use rusqlite::params;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::*;
use crate::acp::limits::LIMITS;

mod rollback;
mod unavailable_binding;

fn database_with_project() -> (Database, String, String) {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let root = std::env::temp_dir().join(format!("sage-bootstrap-chat-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    (database, project.id, chat_id)
}

#[test]
fn browser_layout_persists_and_restores_without_poisoning_chat_projection() {
    let base = std::env::temp_dir().join(format!("sage-browser-layout-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let history = seed_closed_simulator_history(
        &database,
        initial.active_tab_id.as_deref().unwrap(),
        "chat beside browser",
        "preserved chat answer",
    );
    let mut tabs = initial.tabs;
    tabs.push(crate::domain::WorkbenchTab {
        id: "browser-1".into(),
        kind: "browser".into(),
        title: "Browser".into(),
        position: tabs.len() as i64,
        pinned: false,
        dirty: false,
        status: None,
        updated_at: None,
        resource: None,
        repository: None,
        diff_ref: None,
    });
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project.id,
            tabs,
            active_tab_id: Some("browser-1".into()),
            selected_root_id: Some(project.roots[0].id.clone()),
            selected_resource: None,
        })
        .unwrap();
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let snapshot = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.active_tab_id.as_deref(), Some("browser-1"));
    assert!(snapshot.tabs.iter().any(|tab| {
        tab.kind == "browser" && tab.id == "browser-1" && tab.status.as_deref() == Some("closed")
    }));
    assert_eq!(snapshot.session_id, None);
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].session_id, history);
    assert_eq!(snapshot.messages[0].text, "chat beside browser");
    assert_eq!(snapshot.messages[1].text, "preserved chat answer");
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn popup_insertion_reorders_occupied_positions_atomically_and_only_once() {
    let (database, project_id, _) = database_with_project();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let mut request = ProjectLayoutRequest {
        project_id,
        tabs: snapshot.tabs,
        active_tab_id: snapshot.active_tab_id,
        selected_root_id: None,
        selected_resource: None,
    };
    let mut browser = request.tabs[0].clone();
    browser.id = "browser-1".into();
    browser.kind = "browser".into();
    browser.title = "Opener".into();
    browser.position = request.tabs.len() as i64;
    request.tabs.push(browser.clone());
    let mut following = browser.clone();
    following.id = "browser-3".into();
    following.position += 1;
    request.tabs.push(following);
    database.persist_project_layout(&request).unwrap();
    browser.id = "browser-2".into();
    browser.title = "Popup".into();
    request.tabs.insert(request.tabs.len() - 1, browser);
    for (position, tab) in request.tabs.iter_mut().enumerate() {
        tab.position = position as i64;
    }
    request.active_tab_id = Some("browser-2".into());
    database.persist_project_layout(&request).unwrap();
    database.persist_project_layout(&request).unwrap();
    let restored = database.bootstrap_snapshot().unwrap();
    assert_eq!(restored.tabs.len(), request.tabs.len());
    assert_eq!(
        restored
            .tabs
            .iter()
            .map(|tab| (&tab.id, tab.position))
            .collect::<Vec<_>>(),
        request
            .tabs
            .iter()
            .map(|tab| (&tab.id, tab.position))
            .collect::<Vec<_>>()
    );
    request.tabs.reverse();
    for (position, tab) in request.tabs.iter_mut().enumerate() {
        tab.position = position as i64;
    }
    database.persist_project_layout(&request).unwrap();
    assert_eq!(
        database.bootstrap_snapshot().unwrap().tabs[0].id,
        "browser-3"
    );
}

fn seed_closed_simulator_history(
    database: &Database,
    chat_id: &str,
    prompt: &str,
    answer: &str,
) -> String {
    let history = database
        .append_simulator_turn(chat_id, prompt, answer)
        .unwrap();
    let mut connection = database.write_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "DELETE FROM chat_session_heads WHERE session_id=?1",
            [&history.session_id],
        )
        .unwrap();
    let now = now_millis();
    transaction
        .execute(
            "UPDATE agent_sessions SET state='closing',revision=revision+1,updated_at=?2
             WHERE id=?1 AND state='ready'",
            params![history.session_id, now],
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE agent_sessions SET state='closed',revision=revision+1,updated_at=?2,terminal_at=?2
             WHERE id=?1 AND state='closing'",
            params![history.session_id, now],
        )
        .unwrap();
    transaction.commit().unwrap();
    history.session_id
}

fn insert_empty_chat(database: &Database, project_id: &str, chat_id: &str, ordinal: i64) {
    database
        .connection()
        .execute_batch(&format!(
            "INSERT INTO workbench_tabs(
               id,project_id,kind,title,position,pinned,dirty,status,
               metadata_json,created_at,updated_at
             ) VALUES ('{chat_id}','{project_id}','chat','Chat {ordinal}',{ordinal},0,0,'idle','{{}}',2,2);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
             VALUES ('{chat_id}','{project_id}',{ordinal},'Chat {ordinal}',0,2,2);
             INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
             VALUES ('{project_id}','{chat_id}','{chat_id}',2);"
        ))
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE acp_project_usage SET chat_count=chat_count+1,revision=revision+1
             WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE acp_store_usage SET chat_count=chat_count+1,revision=revision+1
             WHERE singleton=1",
            [],
        )
        .unwrap();
}

#[test]
fn active_empty_chat_never_projects_another_chats_head_or_history() {
    let (database, project_id, chat_id) = database_with_project();
    database
        .append_simulator_turn(&chat_id, "other chat prompt", "other chat answer")
        .unwrap();
    insert_empty_chat(&database, &project_id, "chat-empty", 1);
    database
        .connection()
        .execute(
            "UPDATE project_layouts SET active_tab_id='chat-empty' WHERE project_id=?1",
            [&project_id],
        )
        .unwrap();

    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.active_tab_id.as_deref(), Some("chat-empty"));
    assert_eq!(snapshot.session_id, None);
    assert!(snapshot.messages.is_empty());
}

#[test]
fn corrupt_non_chat_attachment_cannot_drive_chat_projection() {
    let (database, _, chat_id) = database_with_project();
    database
        .append_simulator_turn(&chat_id, "attached chat prompt", "attached chat answer")
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE workbench_tabs SET kind='settings' WHERE id=?1",
            [&chat_id],
        )
        .unwrap();

    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.active_tab_id.as_deref(), Some(chat_id.as_str()));
    assert_eq!(snapshot.session_id, None);
    assert!(snapshot.messages.is_empty());
}

#[test]
fn file_active_bootstrap_uses_the_latest_chat_ordinal() {
    let (database, project_id, chat_id) = database_with_project();
    database
        .append_simulator_turn(&chat_id, "older chat prompt", "older chat answer")
        .unwrap();
    insert_empty_chat(&database, &project_id, "chat-latest", 1);
    let root_id: String = database
        .connection()
        .query_row(
            "SELECT id FROM project_roots WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let metadata = serde_json::json!({
        "resource": {"rootId": root_id.clone(), "relativePath": "README.md"}
    })
    .to_string();
    database
        .connection()
        .execute(
            "INSERT INTO workbench_tabs(
               id,project_id,kind,title,position,pinned,dirty,status,
               metadata_json,created_at,updated_at
             ) VALUES ('file-active',?1,'file','README',2,0,0,'idle',?2,3,3)",
            params![project_id, metadata],
        )
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE project_layouts SET active_tab_id='file-active',selected_root_id=?2,
                    selected_relative_path='README.md' WHERE project_id=?1",
            params![project_id, root_id],
        )
        .unwrap();

    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.active_tab_id.as_deref(), Some("file-active"));
    assert_eq!(snapshot.session_id, None);
    assert!(snapshot.messages.is_empty());
}

#[test]
fn restored_binding_identity_maps_through_curated_harness_metadata() {
    let base = std::env::temp_dir().join(format!("sage-binding-restore-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let path = base.join("sage.db");
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(&path).unwrap();
    let project_id = database.create_project_from_root(&root).unwrap().id;
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let history = seed_closed_simulator_history(
        &database,
        &chat_id,
        "simulator history beside Codex",
        "preserved simulator answer",
    );
    let connection = database.connection();
    let payload = r#"{"kind":"text","payload":{"text":"normalized ACP"}}"#;
    let payload_digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
    let payload_bytes = payload.len() as i64;
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,model_id,external_session_id,
               capabilities_json,state,residency,legacy,revision,projection_revision,
               created_at,updated_at,terminal_at
             ) VALUES ('codex-session',?1,?2,1,'codex-app-server',?3,'codex-model','external-codex',
                       ?4,'ready','cold',0,0,2,2,2,NULL)",
            params![
                project_id,
                chat_id,
                "a".repeat(64),
                r#"{"schemaVersion":1,"protocolVersion":"acp-1","newSession":true,"resumeSession":false,"loadSession":false,"closeSession":true,"prompt":true,"cancel":true,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":1,"configCount":0}"#
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,
               started_at,terminal_at,exit_kind
             ) VALUES (?1,?2,'codex-session',1,'exited',NULL,2,2,'test')",
            params![project_id, chat_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
               state,delivery_state,revision,created_at,updated_at,terminal_at
             ) VALUES ('codex-turn',?1,?2,'codex-session',1,0,'codex-intent',
                       'completed','correlated',0,2,2,2)",
            params![project_id, chat_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO items(
               id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,
               lifecycle,payload_json,payload_bytes,payload_digest,revision,
               created_at,updated_at,terminal_at
             ) VALUES ('codex-item',?1,?2,'codex-session','codex-turn',1,0,'primary','text',
                       'completed',?3,?4,?5,0,2,2,2)",
            params![project_id, chat_id, payload, payload_bytes, payload_digest],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO acp_session_usage(
               project_id,chat_id,session_id,turn_count,item_count,item_bytes,
               fact_count,fact_bytes,payload_bytes,revision
             ) VALUES (?1,?2,'codex-session',1,1,?3,0,0,?3,0)",
            params![project_id, chat_id, payload_bytes],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chat_session_heads(
               project_id,chat_id,session_id,installed_revision,installed_at
             ) VALUES (?1,?2,'codex-session',2,2)",
            params![project_id, chat_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE chats SET projection_revision=3 WHERE id=?1",
            [&chat_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET projection_revision=3 WHERE id='codex-session'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE acp_project_usage SET session_count=session_count+1,
                    turn_count=turn_count+1,item_count=item_count+1,
                    payload_bytes=payload_bytes+?2 WHERE project_id=?1",
            params![project_id, payload_bytes],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE acp_store_usage SET session_count=session_count+1,
                    turn_count=turn_count+1,item_count=item_count+1,
                    item_bytes=item_bytes+?1,shared_payload_bytes=shared_payload_bytes+?1
             WHERE singleton=1",
            [payload_bytes],
        )
        .unwrap();
    drop(connection);
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let snapshot = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.session_id.as_deref(), Some("codex-session"));
    assert_eq!(snapshot.selected_harness_id.as_deref(), Some("codex"));
    assert_eq!(snapshot.selected_model_id.as_deref(), Some("codex-model"));
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].session_id, history);
    assert_eq!(snapshot.messages[0].text, "simulator history beside Codex");
    assert_eq!(snapshot.messages[1].text, "preserved simulator answer");
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn closing_chat_tab_detaches_only_projection_and_restart_preserves_history() {
    let base = std::env::temp_dir().join(format!("sage-chat-detach-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let chat_id = initial.active_tab_id.unwrap();
    let chat_tab = initial
        .tabs
        .into_iter()
        .find(|tab| tab.id == chat_id)
        .unwrap();
    let history =
        seed_closed_simulator_history(&database, &chat_id, "persisted prompt", "persisted answer");
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: vec![],
            active_tab_id: None,
            selected_root_id: Some(project.roots[0].id.clone()),
            selected_resource: None,
        })
        .unwrap();
    let durable_before: (i64, i64, i64) = database
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM chats WHERE id=?1),
                    (SELECT COUNT(*) FROM agent_sessions WHERE chat_id=?1),
                    (SELECT COUNT(*) FROM items WHERE chat_id=?1)",
            [&chat_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(durable_before, (1, 1, 2));
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM chat_tabs WHERE chat_id=?1",
                [&chat_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert!(restored.tabs.is_empty());
    assert_eq!(restored.active_tab_id, None);
    assert_eq!(restored.session_id, None);
    assert!(restored.messages.is_empty());
    let durable_after: (i64, i64, i64) = reopened
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM chats WHERE id=?1),
                    (SELECT COUNT(*) FROM agent_sessions WHERE chat_id=?1),
                    (SELECT COUNT(*) FROM items WHERE chat_id=?1)",
            [&chat_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(durable_after, durable_before);
    reopened
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: vec![chat_tab],
            active_tab_id: Some(chat_id.clone()),
            selected_root_id: Some(project.roots[0].id.clone()),
            selected_resource: None,
        })
        .unwrap();
    drop(reopened);

    let reattached = Database::open(&path).unwrap();
    let restored = reattached.bootstrap_snapshot().unwrap();
    assert_eq!(restored.active_tab_id.as_deref(), Some(chat_id.as_str()));
    assert_eq!(restored.session_id, None);
    assert_eq!(restored.messages.len(), 2);
    assert_eq!(restored.messages[0].session_id, history);
    assert_eq!(restored.messages[0].text, "persisted prompt");
    assert_eq!(restored.messages[1].text, "persisted answer");
    let durable_final: (i64, i64, i64) = reattached
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM chats WHERE id=?1),
                    (SELECT COUNT(*) FROM agent_sessions WHERE chat_id=?1),
                    (SELECT COUNT(*) FROM items WHERE chat_id=?1)",
            [&chat_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(durable_final, durable_before);
    drop(reattached);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn creating_a_chat_is_atomic_isolated_and_survives_restart() {
    let base = std::env::temp_dir().join(format!("sage-chat-create-{}", Uuid::new_v4()));
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = initial.active_tab_id.clone().unwrap();
    let first_history = database
        .append_simulator_turn(&first_chat_id, "first prompt", "first response")
        .unwrap();

    let second_chat_id = database
        .create_chat(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: initial.tabs,
            active_tab_id: initial.active_tab_id,
            selected_root_id: initial.selected_root_id,
            selected_resource: initial.selected_resource,
        })
        .unwrap();

    let created = database.bootstrap_snapshot().unwrap();
    assert_ne!(second_chat_id, first_chat_id);
    assert_eq!(
        created.active_tab_id.as_deref(),
        Some(second_chat_id.as_str())
    );
    assert_eq!(created.tabs.len(), 2);
    assert!(created.messages.is_empty());
    database
        .append_simulator_turn(&second_chat_id, "second prompt", "second response")
        .unwrap();
    let first_messages = database.messages_for_chat(&first_chat_id).unwrap();
    assert_eq!(first_messages.len(), 2);
    assert_eq!(first_messages[0].text, "first prompt");
    assert_eq!(first_messages[1].text, "first response");
    assert_eq!(
        database.messages_for_chat(&second_chat_id).unwrap().len(),
        2
    );
    assert_eq!(
        database
            .connection()
            .query_row("SELECT COUNT(*) FROM chat_lineage", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(
        restored.active_tab_id.as_deref(),
        Some(second_chat_id.as_str())
    );
    assert_eq!(restored.tabs.len(), 2);
    assert_eq!(restored.messages.len(), 2);
    let first_tab = restored
        .tabs
        .iter()
        .find(|tab| tab.id == first_chat_id)
        .cloned()
        .unwrap();
    reopened
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project.id,
            tabs: restored.tabs,
            active_tab_id: Some(first_tab.id),
            selected_root_id: restored.selected_root_id,
            selected_resource: restored.selected_resource,
        })
        .unwrap();
    let first_restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(first_restored.session_id, Some(first_history.session_id));
    assert_eq!(first_restored.messages.len(), 2);
    assert_eq!(first_restored.messages[0].text, "first prompt");
    assert_eq!(first_restored.messages[1].text, "first response");
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn durable_chat_capacity_rejects_creation_without_changing_the_layout() {
    let (database, project_id, first_chat_id) = database_with_project();
    for ordinal in 1..LIMITS.durable.chats_per_project as i64 {
        insert_empty_chat(
            &database,
            &project_id,
            &format!("chat-capacity-{ordinal}"),
            ordinal,
        );
    }
    let snapshot = database.bootstrap_snapshot().unwrap();
    let first_tab = snapshot
        .tabs
        .iter()
        .find(|tab| tab.id == first_chat_id)
        .cloned()
        .unwrap();
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project_id.clone(),
            tabs: vec![first_tab.clone()],
            active_tab_id: Some(first_chat_id.clone()),
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        })
        .unwrap();
    let before = database.bootstrap_snapshot().unwrap();

    let error = database
        .create_chat(&ProjectLayoutRequest {
            project_id: project_id.clone(),
            tabs: before.tabs.clone(),
            active_tab_id: before.active_tab_id.clone(),
            selected_root_id: before.selected_root_id.clone(),
            selected_resource: before.selected_resource.clone(),
        })
        .unwrap_err();

    assert!(error.to_string().contains("64-Chat limit"));
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.tabs.len(), 1);
    assert_eq!(after.active_tab_id.as_deref(), Some(first_chat_id.as_str()));
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
                [&project_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        LIMITS.durable.chats_per_project as i64
    );
}
