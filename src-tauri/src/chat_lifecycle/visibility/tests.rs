use crate::chat_lifecycle::create::ChatCreateStore;
use crate::chat_queue::{ChatQueueStore, DurableQueuedPrompt, QueuedPromptStatus};
use crate::chat_search::{ProjectChatSearchRequest, search_project_chats};
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};
use crate::session_events::SessionEventStore;
use crate::workbench_persistence::store::WorkbenchStore;
use rusqlite::params;
use sage_db::database::Database;
use sage_domain::{chat::ProjectLayoutRequest, project::ProjectSwitchRequest};

fn layout(db: &Database) -> ProjectLayoutRequest {
    let s = db.bootstrap_snapshot().unwrap();
    ProjectLayoutRequest {
        project_id: s.project.unwrap().id,
        tabs: s.tabs,
        active_tab_id: s.active_tab_id,
        selected_root_id: s.selected_root_id,
        selected_resource: s.selected_resource,
    }
}
fn create(db: &Database) -> String {
    db.create_chat(&layout(db)).unwrap()
}
fn listed(db: &Database, project: &str) -> Vec<String> {
    db.project_list()
        .unwrap()
        .into_iter()
        .find(|p| p.id == project)
        .unwrap()
        .rail_chats
        .into_iter()
        .map(|c| c.id)
        .collect()
}
fn search(db: &Database, project: &str, query: &str) -> Vec<String> {
    search_project_chats(
        db,
        &ProjectChatSearchRequest {
            draft_chat_ids: Vec::new(),
            project_id: project.into(),
            project_generation: 0,
            expected_transition_epoch: 0,
            query: query.into(),
        },
    )
    .unwrap()
    .results
    .into_iter()
    .map(|c| c.chat_id)
    .collect()
}

#[test]
fn discovery_hides_legacy_empties_before_ranking_limits_and_search_without_deleting() {
    let db = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    let project = db
        .create_named_project_if_active("Fresh", &[], None)
        .unwrap();
    assert!(listed(&db, &project.id).is_empty());
    // More hidden defaults than the rail preview, using both old placeholder titles.
    for index in 0..18 {
        let id = create(&db);
        db.connection()
            .execute(
                "UPDATE chats SET title=?2 WHERE id=?1",
                params![
                    id,
                    if index % 2 == 0 {
                        "Chat"
                    } else {
                        "New chat 56"
                    }
                ],
            )
            .unwrap();
    }
    let real = create(&db);
    db.append_simulator_turn(&real, "real prompt", "answer")
        .unwrap();
    let pinned = create(&db);
    db.connection()
        .execute("UPDATE workbench_tabs SET pinned=1 WHERE id=?1", [&pinned])
        .unwrap();
    let renamed = create(&db);
    db.connection()
        .execute(
            "UPDATE chats SET title='Chosen',title_source='user' WHERE id=?1",
            [&renamed],
        )
        .unwrap();
    let queued = create(&db);
    db.replace_chat_queued_prompts(
        &project.id,
        &queued,
        &[DurableQueuedPrompt {
            id: "queue-1".into(),
            text: "Keep my queued prompt".into(),
            attached_context: Default::default(),
            binding_id: "binding".into(),
            model_id: None,
            status: QueuedPromptStatus::Queued,
        }],
    )
    .unwrap();
    let ids = listed(&db, &project.id);
    assert_eq!(ids.len(), 4);
    for id in [&real, &pinned, &renamed, &queued] {
        assert!(ids.contains(id));
    }
    assert!(!db.project_list().unwrap()[0].rail_chats_truncated);
    let results = search(&db, &project.id, "");
    assert_eq!(results.len(), 4);
    assert!(search(&db, &project.id, "New chat").is_empty());
    assert_eq!(search(&db, &project.id, "real"), vec![real]);
    let retained: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM chats", [], |r| r.get(0))
        .unwrap();
    assert_eq!(retained, 22);
}

#[test]
fn harness_only_and_undone_turns_do_not_count_as_sent_prompts() {
    let db = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    let project = db
        .create_named_project_if_active("Turns", &[], None)
        .unwrap();
    for fact_kind in ["harnessTurnStarted", "turnUndone"] {
        let chat = create(&db);
        db.append_simulator_turn(&chat, "prompt", "answer").unwrap();
        assert_eq!(listed(&db, &project.id), vec![chat.clone()]);
        {
            let mut connection = db.write_connection().unwrap();
            let transaction = connection.transaction().unwrap();
            let (session, turn): (String, String) = transaction
                .query_row(
                    "SELECT session_id,id FROM turns WHERE chat_id=?1 LIMIT 1",
                    [&chat],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            use sha2::{Digest, Sha256};
            transaction
                .execute(
                    "INSERT INTO session_facts(
                project_id,chat_id,session_id,sequence,turn_id,kind,payload_json,
                payload_bytes,payload_digest,projection_revision,created_at)
                SELECT ?1,?2,?3,COALESCE(MAX(sequence),0)+1,?4,?5,'{}',2,?6,2,2
                FROM session_facts WHERE session_id=?3",
                    params![
                        project.id,
                        chat,
                        session,
                        turn,
                        fact_kind,
                        format!("{:x}", Sha256::digest(b"{}"))
                    ],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        assert!(listed(&db, &project.id).is_empty());
        assert!(search(&db, &project.id, "prompt").is_empty());
    }
}

#[test]
fn open_empty_remains_usable_but_switch_and_restart_release_hidden_selection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../local-evidence/emptychat/restore")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("sage.db");
    let db = crate::database::open_database(&path).unwrap();
    let project = db
        .create_named_project_if_active("First", &[], None)
        .unwrap();
    let real = create(&db);
    db.append_simulator_turn(&real, "older real chat", "answer")
        .unwrap();
    let empty = create(&db);
    assert_eq!(
        db.bootstrap_snapshot().unwrap().active_tab_id.as_deref(),
        Some(empty.as_str())
    );
    assert!(
        db.bootstrap_snapshot()
            .unwrap()
            .tabs
            .iter()
            .any(|t| t.id == empty && t.chat_listed == Some(false))
    );
    let other = db
        .create_named_project_if_active("Second", &[], Some(&project.id))
        .unwrap();
    db.switch_project(&ProjectSwitchRequest {
        target_project_id: project.id.clone(),
        current_layout: layout(&db),
    })
    .unwrap();
    assert_eq!(
        db.bootstrap_snapshot().unwrap().active_tab_id.as_deref(),
        Some(real.as_str())
    );
    let mut current = layout(&db);
    current.active_tab_id = Some(empty.clone());
    db.persist_project_layout(&current).unwrap();
    drop(db);
    let db = crate::database::open_database(&path).unwrap();
    assert_eq!(
        db.bootstrap_snapshot().unwrap().active_tab_id.as_deref(),
        Some(real.as_str())
    );
    db.switch_project(&ProjectSwitchRequest {
        target_project_id: other.id.clone(),
        current_layout: layout(&db),
    })
    .unwrap();
    let only_empty = create(&db);
    drop(db);
    let db = crate::database::open_database(&path).unwrap();
    assert!(db.bootstrap_snapshot().unwrap().active_tab_id.is_none());
    let retained: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM chats WHERE id IN (?1,?2)",
            params![empty, only_empty],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retained, 2);
}

#[test]
fn typed_draft_search_exception_is_scoped_bounded_and_does_not_list_other_empty_chats() {
    let db = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    let project = db
        .create_named_project_if_active("Drafts", &[], None)
        .unwrap();
    let draft = create(&db);
    let _hidden = create(&db);
    let mut request = ProjectChatSearchRequest {
        project_id: project.id.clone(),
        project_generation: 0,
        expected_transition_epoch: 0,
        query: "Chat".into(),
        draft_chat_ids: vec![draft.clone()],
    };
    let response = search_project_chats(&db, &request).unwrap();
    assert_eq!(
        response
            .results
            .iter()
            .map(|c| c.chat_id.clone())
            .collect::<Vec<_>>(),
        vec![draft]
    );
    request.draft_chat_ids = vec!["x".into(); 129];
    assert!(search_project_chats(&db, &request).is_err());
    assert!(listed(&db, &project.id).is_empty());
}

#[test]
fn legacy_welcome_only_chat_stays_hidden_without_removing_its_history() {
    let db = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    let project = db
        .create_named_project_if_active("Legacy", &[], None)
        .unwrap();
    let chat = create(&db);
    {
        let mut connection = db.write_connection().unwrap();
        let transaction = connection.transaction().unwrap();
        let now: i64 = transaction
            .query_row("SELECT created_at FROM chats WHERE id=?1", [&chat], |r| {
                r.get(0)
            })
            .unwrap();
        crate::session_events::seed_legacy_welcome(
            &transaction,
            &project.id,
            &chat,
            "welcome-session",
            now,
            r#"{"role":"assistant","text":"Historical onboarding"}"#,
            "simulatorTurnCommitted",
        )
        .unwrap();
        transaction.commit().unwrap();
    }
    assert!(listed(&db, &project.id).is_empty());
    assert!(search(&db, &project.id, "").is_empty());
    let retained: (i64, i64) = db
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM chats),(SELECT COUNT(*) FROM turns)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(retained, (1, 1));
    db.append_simulator_turn(&chat, "A real first prompt", "answer")
        .unwrap();
    assert_eq!(listed(&db, &project.id), vec![chat]);
}

#[test]
fn foreign_first_send_creation_keeps_its_explicit_selection_over_an_older_real_chat() {
    let db = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    let target = db
        .create_named_project_if_active("Target", &[], None)
        .unwrap();
    let previous = create(&db);
    db.append_simulator_turn(&previous, "Previous real chat", "answer")
        .unwrap();
    let source = db
        .create_named_project_if_active("Source", &[], Some(&target.id))
        .unwrap();
    let identity = uuid::Uuid::new_v4().to_string();
    let created = db
        .create_chat_with_identity(&target.id, &layout(&db), None, Some(&identity))
        .unwrap();
    assert_eq!(created, identity);
    let snapshot = db.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.project.unwrap().id, target.id);
    assert_eq!(snapshot.active_tab_id, Some(identity.clone()));
    assert!(snapshot.tabs.iter().any(|tab| tab.id == identity));
    assert_ne!(source.id, target.id);
    db.append_simulator_turn(&identity, "First prompt from another project", "answer")
        .unwrap();
    assert_eq!(listed(&db, &target.id).len(), 2);
}
