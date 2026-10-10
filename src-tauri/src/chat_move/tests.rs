use std::fs;

use crate::{
    domain::{MoveChatRequest, ProjectLayoutRequest, ProjectSwitchRequest},
    error::AppError,
    persistence::Database,
};

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

#[test]
fn moves_an_attached_chat_to_another_project() {
    let base = std::env::temp_dir().join(format!("sage-chat-move-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("first");
    let second_root = base.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let first = database.create_project_from_root(&first_root).unwrap();
    let first_chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let second_chat = database
        .create_chat(&layout(&database.bootstrap_snapshot().unwrap()))
        .unwrap();
    let second = database.create_project_from_root(&second_root).unwrap();
    database
        .switch_project(&ProjectSwitchRequest {
            current_layout: layout(&database.bootstrap_snapshot().unwrap()),
            target_project_id: first.id.clone(),
        })
        .unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    database
        .move_chat(&MoveChatRequest {
            project_id: first.id.clone(),
            chat_id: second_chat.clone(),
            target_project_id: second.id.clone(),
            current_layout: layout(&before),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, second.id);
    assert!(after.tabs.iter().any(|tab| tab.id == second_chat));
    assert!(!after.tabs.iter().any(|tab| tab.id == first_chat));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn moves_only_a_proven_legacy_welcome_and_preserves_rows_accounting_and_guards() {
    for (fact_kind, inject_failure) in [
        ("simulatorTurnCommitted", false),
        ("observedTurn", false),
        ("simulatorTurnCommitted", true),
    ] {
        let base =
            std::env::temp_dir().join(format!("sage-chat-move-welcome-{}", uuid::Uuid::new_v4()));
        let first_root = base.join("first");
        let second_root = base.join("second");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let first = database.create_project_from_root(&first_root).unwrap();
        let chat_id = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        {
            let mut connection = database.write_connection().unwrap();
            let transaction = connection.transaction().unwrap();
            let now: i64 = transaction
                .query_row(
                    "SELECT created_at FROM chats WHERE id=?1",
                    [&chat_id],
                    |row| row.get(0),
                )
                .unwrap();
            crate::session_events::seed_legacy_welcome(
                &transaction,
                &first.id,
                &chat_id,
                &session_id,
                now,
                r#"{"role":"assistant","text":"ready to choose a harness"}"#,
                fact_kind,
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        let second = database.create_project_from_root(&second_root).unwrap();
        if inject_failure {
            database
                .connection()
                .execute_batch(
                    "CREATE TRIGGER fail_welcome_move BEFORE UPDATE OF project_id ON items
                 BEGIN SELECT RAISE(ABORT,'injected welcome move failure'); END;",
                )
                .unwrap();
        }
        let project_ledgers = || {
            database.connection().query_row(
            "SELECT json_group_array(json_array(project_id,chat_count,session_count,turn_count,
                                                item_count,fact_count,payload_bytes,revision))
             FROM acp_project_usage",
            [], |row| row.get::<_, String>(0),
        ).unwrap()
        };
        let projects_before = project_ledgers();
        let ledgers =
            || {
                database.connection().query_row(
                "SELECT json_array(chat_count,session_count,turn_count,item_count,item_bytes,
                                   fact_count,fact_bytes,shared_payload_bytes,revision)
                 FROM acp_store_usage WHERE singleton=1",
                [], |row| row.get::<_, String>(0),
            ).unwrap()
            };
        let guards = || {
            let connection = database.connection();
            let mut statement = connection
                .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' ORDER BY name")
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let ledger_before = ledgers();
        let guards_before = guards();
        let result = database.move_chat(&MoveChatRequest {
            project_id: first.id.clone(),
            chat_id: chat_id.clone(),
            target_project_id: second.id.clone(),
            current_layout: layout(&database.bootstrap_snapshot().unwrap()),
        });
        if inject_failure {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("injected welcome move failure")
            );
            assert_eq!(project_ledgers(), projects_before);
            let connection = database.connection();
            let owners: (String, String) = connection.query_row(
                "SELECT c.project_id,s.project_id FROM chats c JOIN agent_sessions s ON s.chat_id=c.id
                 WHERE c.id=?1 AND s.id=?2",
                rusqlite::params![chat_id, session_id], |row| Ok((row.get(0)?,row.get(1)?)),
            ).unwrap();
            assert_eq!(owners, (first.id.clone(), first.id.clone()));
            assert!(
                connection
                    .execute(
                        "UPDATE agent_sessions SET project_id=?2 WHERE id=?1",
                        rusqlite::params![session_id, second.id],
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("identity is immutable")
            );
        } else if fact_kind == "observedTurn" {
            assert!(matches!(result, Err(AppError::InvalidRequest(_))));
        } else {
            result.unwrap();
            let after = database.bootstrap_snapshot().unwrap();
            assert!(after.tabs.iter().any(|tab| tab.id == chat_id));
            assert_eq!(database.messages_for_chat(&chat_id).unwrap().len(), 0);
            let connection = database.connection();
            let owner: String = connection
                .query_row(
                    "SELECT project_id FROM agent_sessions WHERE id=?1",
                    [&session_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(owner, second.id);
            assert!(
                crate::session_events::legacy_welcome::turn_id(&connection, &chat_id)
                    .unwrap()
                    .is_some()
            );
            for (project, expected) in [(&first.id, (0, 0, 0, 0, 0)), (&second.id, (2, 1, 1, 1, 1))]
            {
                let counts: (i64, i64, i64, i64, i64) = connection
                    .query_row(
                        "SELECT chat_count,session_count,turn_count,item_count,fact_count
                     FROM acp_project_usage WHERE project_id=?1",
                        [project],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                            ))
                        },
                    )
                    .unwrap();
                assert_eq!(counts, expected);
                let bytes: (i64, i64) = connection.query_row(
                    "SELECT p.payload_bytes,COALESCE(SUM(u.payload_bytes),0)
                     FROM acp_project_usage p LEFT JOIN acp_session_usage u ON u.project_id=p.project_id
                     WHERE p.project_id=?1", [project], |row| Ok((row.get(0)?, row.get(1)?)),
                ).unwrap();
                assert_eq!(bytes.0, bytes.1);
            }
            assert_eq!(
                connection
                    .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                0
            );
            drop(connection);
        }
        assert_eq!(ledgers(), ledger_before);
        assert_eq!(guards(), guards_before);
        fs::remove_dir_all(base).unwrap();
    }
}

#[test]
fn refuses_same_project_and_unknown_target() {
    let root = std::env::temp_dir().join(format!("sage-chat-move-deny-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let chat_id = snapshot.active_tab_id.clone().unwrap();
    let same = database
        .move_chat(&MoveChatRequest {
            project_id: project.id.clone(),
            chat_id: chat_id.clone(),
            target_project_id: project.id.clone(),
            current_layout: layout(&snapshot),
        })
        .unwrap_err();
    assert!(matches!(same, AppError::InvalidRequest(_)));
    let unknown = database
        .move_chat(&MoveChatRequest {
            project_id: project.id,
            chat_id,
            target_project_id: "missing-project".into(),
            current_layout: layout(&snapshot),
        })
        .unwrap_err();
    assert!(matches!(unknown, AppError::InvalidRequest(_)));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn moves_a_sessionless_chat_from_a_non_active_project_without_switching() {
    let base =
        std::env::temp_dir().join(format!("sage-chat-move-foreign-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("work");
    let third_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    fs::create_dir_all(&third_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let cross_chat = database
        .create_chat(&layout(&database.bootstrap_snapshot().unwrap()))
        .unwrap();
    let work = database.create_project_from_root(&second_root).unwrap();
    let sage = database.create_project_from_root(&third_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    database
        .move_chat(&MoveChatRequest {
            project_id: cross.id.clone(),
            chat_id: cross_chat.clone(),
            target_project_id: work.id.clone(),
            current_layout: layout(&before),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    let work_item = after
        .projects
        .iter()
        .find(|item| item.id == work.id)
        .unwrap();
    assert!(
        work_item
            .rail_chats
            .iter()
            .any(|chat| chat.id == cross_chat)
    );
    let cross_item = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap();
    assert!(
        !cross_item
            .rail_chats
            .iter()
            .any(|chat| chat.id == cross_chat)
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn refuses_to_move_a_chat_that_has_an_agent_session() {
    let base =
        std::env::temp_dir().join(format!("sage-chat-move-session-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("first");
    let second_root = base.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let first = database.create_project_from_root(&first_root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    database
        .append_simulator_turn(&chat_id, "keep this chat here", "session exists")
        .unwrap();
    let second = database.create_project_from_root(&second_root).unwrap();
    let error = database
        .move_chat(&MoveChatRequest {
            project_id: first.id,
            chat_id,
            target_project_id: second.id,
            current_layout: layout(&database.bootstrap_snapshot().unwrap()),
        })
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidRequest(_)));
    assert!(error.to_string().contains("agent session"));
    fs::remove_dir_all(base).unwrap();
}
