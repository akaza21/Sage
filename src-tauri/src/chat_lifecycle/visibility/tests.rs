use crate::chat_lifecycle::create::ChatCreateStore;
use crate::chat_queue::{ChatQueueStore, DurableQueuedPrompt, QueuedPromptStatus};
use crate::chat_search::{ProjectChatSearchRequest, search_project_chats};
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};
use crate::session_events::SessionEventStore;
use crate::workbench_persistence::store::WorkbenchStore;
use rusqlite::params;
use sage_db::database::Database;
use sage_domain::{chat::ProjectLayoutRequest, project::ProjectSwitchRequest};

// Declared before database handles so unwinding closes SQLite before cleanup.
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("chat-visibility-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove visibility test directory");
    }
}

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
    let directory = TestDirectory::new();
    let path = directory.0.join("sage.db");
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

#[test]
fn upgrade_at_capacity_reuses_the_oldest_eligible_chat_without_growing() {
    let db = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
    let project = db
        .create_named_project_if_active("Upgrade", &[], None)
        .unwrap();
    let ids: Vec<_> = (0..64).map(|_| create(&db)).collect();
    assert!(listed(&db, &project.id).is_empty());
    // Local typed input protects the oldest chat; native pinning, renaming,
    // queued input, archives and any previous prompt protect the others.
    db.connection()
        .execute("UPDATE workbench_tabs SET pinned=1 WHERE id=?1", [&ids[1]])
        .unwrap();
    db.connection()
        .execute(
            "UPDATE chats SET title_source='user' WHERE id=?1",
            [&ids[2]],
        )
        .unwrap();
    db.append_simulator_turn(&ids[3], "Existing prompt", "Answer")
        .unwrap();
    db.connection()
        .execute(
            "UPDATE chats SET title_source='default' WHERE id=?1",
            [&ids[3]],
        )
        .unwrap();
    db.replace_chat_queued_prompts(
        &project.id,
        &ids[4],
        &[DurableQueuedPrompt {
            id: "queued".into(),
            text: "Keep this".into(),
            attached_context: Default::default(),
            binding_id: "binding".into(),
            model_id: None,
            status: QueuedPromptStatus::Queued,
        }],
    )
    .unwrap();
    let selected = super::reusable_chat(&db.connection(), &project.id, &[ids[0].clone()], None)
        .unwrap()
        .unwrap();
    assert_eq!(selected, ids[5]);
    let reused = db
        .reuse_empty_chat(&project.id, &layout(&db), &selected)
        .unwrap();
    assert_eq!(reused, selected);
    db.append_simulator_turn(&reused, "First send after upgrade", "Answer")
        .unwrap();
    let snapshot = db.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.active_tab_id.as_deref(), Some(reused.as_str()));
    assert_eq!(snapshot.tabs.len(), 64);
    let count: i64 = db
        .connection()
        .query_row(
            "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
            [&project.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 64);
    assert!(listed(&db, &project.id).contains(&reused));
}

#[test]
fn pinned_empty_archive_restore_restart_and_switch_keeps_retention() {
    use crate::chat_archive::ChatArchiveStore;
    use crate::chat_pin::ChatPinStore;
    use sage_domain::chat::{ChatLifecycleRequest, PinChatRequest};
    let directory = TestDirectory::new();
    let path = directory.0.join("sage.db");
    let db = crate::database::open_database(&path).unwrap();
    let project = db
        .create_named_project_if_active("Archive", &[], None)
        .unwrap();
    let chat = create(&db);
    db.pin_chat(&PinChatRequest {
        project_id: project.id.clone(),
        chat_id: chat.clone(),
    })
    .unwrap();
    db.archive_chat(&ChatLifecycleRequest {
        project_id: project.id.clone(),
        chat_id: chat.clone(),
        current_layout: layout(&db),
    })
    .unwrap();
    drop(db);
    let db = crate::database::open_database(&path).unwrap();
    assert!(
        db.bootstrap_snapshot()
            .unwrap()
            .archived_chats
            .iter()
            .any(|c| c.id == chat && c.chat_listed == Some(true))
    );
    assert_eq!(search(&db, &project.id, ""), vec![chat.clone()]);
    assert!(
        super::reusable_chat(&db.connection(), &project.id, &[], None)
            .unwrap()
            .is_none()
    );
    db.restore_chat(&ChatLifecycleRequest {
        project_id: project.id.clone(),
        chat_id: chat.clone(),
        current_layout: layout(&db),
    })
    .unwrap();
    let restored = db.bootstrap_snapshot().unwrap();
    assert_eq!(restored.active_tab_id, Some(chat.clone()));
    assert!(restored.tabs.iter().any(|tab| tab.id == chat));
    assert!(restored.archived_chats.is_empty());
    assert!(restored.tabs.iter().any(|tab| tab.id == chat && tab.pinned));
    drop(db);
    let db = crate::database::open_database(&path).unwrap();
    db.create_named_project_if_active("Other", &[], Some(&project.id))
        .unwrap();
    assert_eq!(listed(&db, &project.id), vec![chat.clone()]);
    assert_eq!(search(&db, &project.id, ""), vec![chat.clone()]);
    db.switch_project(&ProjectSwitchRequest {
        target_project_id: project.id.clone(),
        current_layout: layout(&db),
    })
    .unwrap();
    assert_eq!(
        db.bootstrap_snapshot().unwrap().active_tab_id,
        Some(chat.clone())
    );
    assert_eq!(listed(&db, &project.id), vec![chat.clone()]);
    assert_eq!(search(&db, &project.id, ""), vec![chat.clone()]);
    assert!(
        super::reusable_chat(&db.connection(), &project.id, &[], None)
            .unwrap()
            .is_none()
    );
    assert!(
        db.reuse_empty_chat(&project.id, &layout(&db), &chat)
            .is_err()
    );
}

#[test]
fn fork_title_or_native_lineage_remains_discoverable_after_switch_and_restart() {
    let directory = TestDirectory::new();
    let path = directory.0.join("sage.db");
    let db = crate::database::open_database(&path).unwrap();
    let project = db
        .create_named_project_if_active("Fork", &[], None)
        .unwrap();
    let parent = create(&db);
    db.append_simulator_turn(&parent, "Parent prompt", "Answer")
        .unwrap();
    let titled = create(&db);
    db.connection()
        .execute(
            "UPDATE chats SET title_source='fork' WHERE id=?1",
            [&titled],
        )
        .unwrap();
    let lineage = create(&db);
    db.connection().execute(
        "INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,parent_chat_id,source_session_id,source_turn_id,relation,created_at)
         SELECT project_id,?1,project_id,chat_id,session_id,id,'nativeFork',1 FROM turns WHERE chat_id=?2 LIMIT 1",
        params![lineage,parent],
    ).unwrap();
    db.create_named_project_if_active("Other", &[], Some(&project.id))
        .unwrap();
    db.switch_project(&ProjectSwitchRequest {
        target_project_id: project.id.clone(),
        current_layout: layout(&db),
    })
    .unwrap();
    assert_eq!(
        db.bootstrap_snapshot().unwrap().active_tab_id,
        Some(lineage.clone())
    );
    drop(db);
    let db = crate::database::open_database(&path).unwrap();
    for id in [titled, lineage] {
        assert!(listed(&db, &project.id).contains(&id));
        assert!(search(&db, &project.id, "Chat").contains(&id));
    }
}
