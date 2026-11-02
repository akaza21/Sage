use std::{fs, path::Path, time::Duration};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{ProjectChatSearchRequest, SearchLimits, search_project_chats};
use crate::{domain::ProjectLayoutRequest, persistence::Database};

fn database_with_project() -> (Database, String, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-chat-search-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    (database, project.id, root)
}

fn request(project_id: &str, query: &str) -> ProjectChatSearchRequest {
    ProjectChatSearchRequest {
        project_id: project_id.into(),
        project_generation: 7,
        expected_transition_epoch: 3,
        query: query.into(),
    }
}

#[test]
fn search_excludes_only_proven_legacy_welcome_text_and_keeps_real_answers() {
    for fact_kind in ["simulatorTurnCommitted", "observedTurn"] {
        let (database, project_id, root) = database_with_project();
        let chat_id = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
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
                &project_id,
                &chat_id,
                &Uuid::new_v4().to_string(),
                now,
                r#"{"role":"assistant","text":"ready to choose a harness"}"#,
                fact_kind,
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        for query in ["ready", "harness"] {
            let results = search_project_chats(&database, &request(&project_id, query))
                .unwrap()
                .results;
            if fact_kind == "simulatorTurnCommitted" {
                assert!(results.is_empty());
            } else {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].chat_id, chat_id);
                assert_eq!(
                    results[0].snippet.as_deref(),
                    Some("ready to choose a harness")
                );
            }
        }
        database
            .append_simulator_turn(&chat_id, "Real prompt", "Real answer: ready harness")
            .unwrap();
        for query in ["ready", "harness"] {
            let results = search_project_chats(&database, &request(&project_id, query))
                .unwrap()
                .results;
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].chat_id, chat_id);
            assert_eq!(
                results[0].snippet.as_deref(),
                Some("Real answer: ready harness")
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn literal_search_finds_attached_and_archived_primary_text_without_tool_payloads() {
    let (database, project_id, root) = database_with_project();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let first_chat = snapshot.active_tab_id.unwrap();
    let layout = ProjectLayoutRequest {
        project_id: project_id.clone(),
        tabs: snapshot.tabs,
        active_tab_id: Some(first_chat.clone()),
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    };
    let second_chat = database.create_chat(&layout).unwrap();
    database
        .append_simulator_turn_for_model(
            &first_chat,
            "sage-simulator-default",
            "find 100%_literal Ω",
            "answer",
        )
        .unwrap();
    database
        .append_simulator_turn_for_model(
            &second_chat,
            "sage-simulator-default",
            "another 100%_literal Ω",
            "answer",
        )
        .unwrap();
    database
        .connection()
        .execute("DELETE FROM chat_tabs WHERE chat_id=?1", [&second_chat])
        .unwrap();
    {
        let connection = database.connection();
        let (session, turn, epoch, ordinal, now): (String, String, i64, i64, i64) = connection
            .query_row(
                "SELECT session_id,turn_id,runtime_epoch,MAX(ordinal)+1,MAX(created_at)
                 FROM items WHERE chat_id=?1",
                [&first_chat],
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
        let tool_payload = r#"{"kind":"toolCall","payload":{"category":"command","label":"forbidden secret","inputSummary":null}}"#;
        let digest = format!("{:x}", Sha256::digest(tool_payload.as_bytes()));
        connection
            .execute(
                "INSERT INTO items(
                   id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,
                   kind,lifecycle,tool_state,payload_json,payload_bytes,payload_digest,
                   revision,created_at,updated_at,terminal_at
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,'primary','toolCall','completed',
                          'completed',?8,?9,?10,0,?11,?11,?11)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    project_id,
                    first_chat,
                    session,
                    turn,
                    epoch,
                    ordinal,
                    tool_payload,
                    tool_payload.len() as i64,
                    digest,
                    now
                ],
            )
            .unwrap();
    }

    let response = search_project_chats(&database, &request(&project_id, "%_literal Ω")).unwrap();

    assert_eq!(response.results.len(), 2);
    assert!(response.results.iter().any(|result| result.attached));
    assert!(response.results.iter().any(|result| !result.attached));
    assert!(response.results.iter().all(|result| {
        result
            .snippet
            .as_deref()
            .is_some_and(|snippet| snippet.contains("100%_literal Ω"))
    }));
    assert!(
        search_project_chats(&database, &request(&project_id, "forbidden secret"))
            .unwrap()
            .results
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn archived_primary_text_remains_searchable_after_restart() {
    let directory = std::env::temp_dir().join(format!("sage-chat-search-db-{}", Uuid::new_v4()));
    let project_root = directory.join("project");
    fs::create_dir_all(&project_root).unwrap();
    let database_path = directory.join("sage.db");
    let (project_id, chat_id) = {
        let database = Database::open(&database_path).unwrap();
        let project = database.create_project_from_root(&project_root).unwrap();
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        database
            .append_simulator_turn_for_model(
                &chat,
                "sage-simulator-default",
                "restart-durable needle",
                "answer",
            )
            .unwrap();
        database
            .connection()
            .execute("DELETE FROM chat_tabs WHERE chat_id=?1", [&chat])
            .unwrap();
        (project.id, chat)
    };

    let reopened = Database::open(&database_path).unwrap();
    let response =
        search_project_chats(&reopened, &request(&project_id, "restart-durable")).unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].chat_id, chat_id);
    assert!(!response.results[0].attached);
    drop(reopened);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn utf8_query_accepts_exact_limit_and_rejects_one_byte_more() {
    let (database, project_id, root) = database_with_project();
    assert!(search_project_chats(&database, &request(&project_id, &"é".repeat(128))).is_ok());
    let error = search_project_chats(&database, &request(&project_id, &("é".repeat(128) + "a")))
        .unwrap_err();
    assert!(error.to_string().contains("256-byte"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn result_and_scan_limits_are_truthful_and_fail_closed() {
    let (database, project_id, root) = database_with_project();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    database
        .append_simulator_turn(&chat_id, "scan fixture prompt", "scan fixture answer")
        .unwrap();
    {
        let connection = database.connection();
        connection
            .execute(
                "UPDATE chats SET title='needle' WHERE project_id=?1",
                [&project_id],
            )
            .unwrap();
        for ordinal in 1..65_i64 {
            let id = format!("chat-search-{ordinal}");
            connection
                .execute(
                    "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                     VALUES(?1,?2,?3,'needle',0,?3,?3)",
                    rusqlite::params![id, project_id, ordinal],
                )
                .unwrap();
        }
    }
    // Exercise size limits independently of the production wall-clock deadline.
    let limits = SearchLimits {
        deadline: Duration::from_secs(60),
        ..SearchLimits::default()
    };
    let response =
        super::search_with_limits(&database, &request(&project_id, "needle"), limits).unwrap();
    assert_eq!(response.results.len(), 64);
    assert!(response.has_more);

    let error = super::search_with_limits(
        &database,
        &request(&project_id, "needle"),
        SearchLimits {
            max_item_rows: 0,
            ..limits
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("row ceiling"));

    let bytes = super::search_with_limits(
        &database,
        &request(&project_id, "needle"),
        SearchLimits {
            max_item_bytes: 0,
            ..limits
        },
    )
    .unwrap_err();
    assert!(bytes.to_string().contains("byte ceiling"));

    let response_budget = super::search_with_limits(
        &database,
        &request(&project_id, "needle"),
        SearchLimits {
            max_response_bytes: 512,
            ..limits
        },
    )
    .unwrap();
    assert!(response_budget.has_more);
    assert!(serde_json::to_vec(&response_budget).unwrap().len() <= 512);

    let deadline = super::search_with_limits(
        &database,
        &request(&project_id, "needle"),
        SearchLimits {
            deadline: Duration::ZERO,
            ..SearchLimits::default()
        },
    )
    .unwrap_err();
    assert!(deadline.to_string().contains("deadline"));
    fs::remove_dir_all(root).unwrap();
}
