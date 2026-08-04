use std::{fs, path::Path, sync::mpsc, thread, time::Duration};

use uuid::Uuid;

use super::*;
use crate::domain::{ProjectLayoutRequest, ProjectSwitchRequest};

fn session(database: &Database) -> String {
    let root = std::env::temp_dir().join(format!("sage-events-root-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
    database
        .connection()
        .query_row(
            "SELECT id FROM agent_sessions WHERE project_id=?1",
            [project.id],
            |row| row.get(0),
        )
        .unwrap()
}

fn chat(database: &Database, session_id: &str) -> String {
    database
        .connection()
        .query_row(
            "SELECT chat_id FROM agent_sessions WHERE id=?1",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn simulator_seed_and_turn_are_v5_and_ledgers_remain_exact() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let appended = database
        .append_simulator_turn(&chat_id, "hello", "ack")
        .unwrap();
    let session_id = appended.session_id;
    let messages = appended.messages;
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1].role, "user");
    assert_eq!(messages[2].role, "assistant");
    let connection = database.connection();
    let durable: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM turns WHERE session_id=?1),
                    (SELECT COUNT(*) FROM items WHERE session_id=?1),
                    (SELECT COUNT(*) FROM session_facts WHERE session_id=?1),
                    (SELECT COUNT(*) FROM session_runtime_epochs
                     WHERE session_id=?1 AND state='exited' AND terminal_at IS NOT NULL)",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    let ledger: (i64, i64, i64) = connection
        .query_row(
            "SELECT turn_count,item_count,fact_count FROM acp_session_usage WHERE session_id=?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(durable, (1, 2, 1, 1));
    assert_eq!(ledger, (durable.0, durable.1, durable.2));
    let shape: (String, String, i64) = connection
        .query_row(
            "SELECT state,residency,legacy FROM agent_sessions WHERE id=?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(shape, ("ready".into(), "cold".into(), 0));
    let welcome_shape: (String, String, i64, i64) = connection
        .query_row(
            "SELECT state,residency,legacy,
                    (SELECT COUNT(*) FROM chat_session_heads WHERE session_id=?1)
             FROM agent_sessions WHERE id=?1",
            [&welcome_session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(welcome_shape, ("closed".into(), "cold".into(), 0, 0));
    let capabilities: String = connection
        .query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
            [&session_id],
            |row| row.get(0),
        )
        .unwrap();
    let capabilities: serde_json::Value = serde_json::from_str(&capabilities).unwrap();
    assert_eq!(capabilities["protocolVersion"], "sage-simulator-v1");
    assert_eq!(capabilities["prompt"], true);
    assert_eq!(capabilities["modelCount"], 2);
    assert_ledgers_exact(&connection);
}

#[test]
fn fresh_project_welcome_is_closed_and_first_prompt_lazily_installs_head() {
    let base = std::env::temp_dir().join(format!("sage-fresh-chat-v5-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let initial = database.bootstrap_snapshot().unwrap();
    assert_eq!(initial.session_id, None);
    assert_eq!(initial.messages.len(), 1);
    assert_eq!(initial.messages[0].session_id, welcome_session_id);
    let welcome_shape: (String, String, i64, i64, i64) = database
        .connection()
        .query_row(
            "SELECT state,residency,legacy,ordinal,
                    (SELECT COUNT(*) FROM chat_session_heads WHERE chat_id=?2)
             FROM agent_sessions WHERE id=?1",
            rusqlite::params![welcome_session_id, chat_id],
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
    assert_eq!(welcome_shape, ("closed".into(), "cold".into(), 0, 0, 0));

    let appended = database
        .append_simulator_turn(&chat_id, "first prompt", "ack")
        .unwrap();
    assert_ne!(appended.session_id, welcome_session_id);
    assert_eq!(appended.messages.len(), 3);
    let current_shape: (String, String, i64, i64, i64) = database
        .connection()
        .query_row(
            "SELECT state,residency,legacy,ordinal,
                    (SELECT COUNT(*) FROM chat_session_heads WHERE session_id=?1)
             FROM agent_sessions WHERE id=?1",
            [&appended.session_id],
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
    assert_eq!(current_shape, ("ready".into(), "cold".into(), 0, 1, 1));
    let session_id = appended.session_id;
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(restored.session_id, Some(session_id));
    assert_eq!(restored.messages.len(), 3);
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn simulator_model_identity_restores_reuses_and_replaces_attempts() {
    let base = std::env::temp_dir().join(format!("sage-simulator-model-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let deep = database
        .append_simulator_turn_for_model(&chat_id, "sim-deep", "deep", "ack")
        .unwrap();
    let deep_session_id = deep.session_id;
    let deep_model: String = database
        .connection()
        .query_row(
            "SELECT model_id FROM agent_sessions WHERE id=?1",
            [&deep_session_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(deep_model, "sim-deep");
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(
        restored.session_id.as_deref(),
        Some(deep_session_id.as_str())
    );
    assert_eq!(restored.selected_model_id.as_deref(), Some("sim-deep"));
    let reused = reopened
        .append_simulator_turn_for_model(&chat_id, "sim-deep", "deep again", "ack")
        .unwrap();
    assert_eq!(reused.session_id, deep_session_id);

    let fast = reopened
        .append_simulator_turn_for_model(&chat_id, "sim-fast", "fast", "ack")
        .unwrap();
    assert_ne!(fast.session_id, deep_session_id);
    let shapes = {
        let connection = reopened.connection();
        let mut statement = connection
            .prepare(
                "SELECT id,ordinal,model_id,state FROM agent_sessions
                 WHERE chat_id=?1 ORDER BY ordinal",
            )
            .unwrap();
        statement
            .query_map([&chat_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(shapes.len(), 3);
    assert_eq!(shapes[0].1, 0);
    assert_eq!(shapes[0].3, "closed");
    assert_eq!(
        shapes[1],
        (deep_session_id, 1, Some("sim-deep".into()), "closed".into())
    );
    assert_eq!(
        shapes[2],
        (
            fast.session_id.clone(),
            2,
            Some("sim-fast".into()),
            "ready".into()
        )
    );
    assert_eq!(fast.messages.len(), 7);
    drop(reopened);

    let restarted = Database::open(&path).unwrap();
    let restored = restarted.bootstrap_snapshot().unwrap();
    assert_eq!(restored.session_id, Some(fast.session_id));
    assert_eq!(restored.selected_model_id.as_deref(), Some("sim-fast"));
    drop(restarted);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn simulator_model_replacement_and_turn_roll_back_as_one_transaction() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let deep = database
        .append_simulator_turn_for_model(&chat_id, "sim-deep", "deep", "ack")
        .unwrap();
    let before = durable_chat_state(&database, &chat_id);
    let before_messages = deep
        .messages
        .iter()
        .map(|message| {
            (
                message.id.clone(),
                message.session_id.clone(),
                message.sequence,
                message.text.clone(),
            )
        })
        .collect::<Vec<_>>();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        database.append_simulator_turn_for_model_with_hook(
            &chat_id,
            "sim-fast",
            "fast",
            "ack",
            || panic!("injected replacement interruption"),
        )
    }));
    assert!(result.is_err());
    assert_eq!(durable_chat_state(&database, &chat_id), before);
    let head: (String, String, String) = database
        .connection()
        .query_row(
            "SELECT s.id,s.model_id,s.state FROM chat_session_heads h
             JOIN agent_sessions s ON s.id=h.session_id WHERE h.chat_id=?1",
            [&chat_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        head,
        (deep.session_id.clone(), "sim-deep".into(), "ready".into())
    );
    let restored_messages = database
        .messages(&deep.session_id)
        .unwrap()
        .into_iter()
        .map(|message| {
            (
                message.id,
                message.session_id,
                message.sequence,
                message.text,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(restored_messages, before_messages);
    assert_ledgers_exact(&database.connection());
}

#[test]
fn full_session_ledger_blocks_new_turn_without_touching_history() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let session_id = database
        .append_simulator_turn(&chat_id, "first", "ack")
        .unwrap()
        .session_id;
    let before: (i64, i64, i64) = database
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM turns WHERE session_id=?1),
                    (SELECT COUNT(*) FROM items WHERE session_id=?1),
                    (SELECT COUNT(*) FROM session_facts WHERE session_id=?1)",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    database
        .connection()
        .execute(
            "UPDATE acp_session_usage SET turn_count=?1 WHERE session_id=?2",
            rusqlite::params![
                crate::acp::limits::LIMITS.durable.turns_per_session as i64,
                &session_id
            ],
        )
        .unwrap();
    assert!(
        database
            .append_simulator_turn(&chat_id, "new", "ack")
            .is_err()
    );
    let after: (i64, i64, i64) = database
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM turns WHERE session_id=?1),
                    (SELECT COUNT(*) FROM items WHERE session_id=?1),
                    (SELECT COUNT(*) FROM session_facts WHERE session_id=?1)",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn simulator_preserves_ade0005_capacity_boundary_after_v5() {
    let base = std::env::temp_dir().join(format!("sage-capacity-v5-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let database_path = base.join("sage.db");
    let database = Database::open(&database_path).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);

    let mut session_id = None;
    let mut last_sequence = 0;
    for ordinal in 0..2_500 {
        let appended = database
            .append_simulator_turn(&chat_id, &format!("prompt-{ordinal}"), "ack")
            .unwrap();
        session_id.get_or_insert_with(|| appended.session_id.clone());
        last_sequence = appended.messages.last().unwrap().sequence;
    }
    let session_id = session_id.unwrap();
    assert_eq!(last_sequence, 5_001);
    let durable_items: i64 = database
        .connection()
        .query_row(
            "SELECT item_count FROM acp_session_usage WHERE session_id=?1",
            [&session_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(durable_items, 5_000);
    assert!(matches!(
        database.append_simulator_turn(&chat_id, "over-limit", "ack"),
        Err(AppError::ResourceLimit(_))
    ));

    drop(database);
    let reopened = Database::open(&database_path).unwrap();
    let projection = reopened.messages(&session_id).unwrap();
    assert_eq!(projection.len(), MAX_BOOTSTRAP_MESSAGES as usize);
    assert_eq!(projection.last().unwrap().sequence, 5_001);
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn native_reopen_is_idempotent_for_simulator_projection_and_ledgers() {
    let base = std::env::temp_dir().join(format!("sage-simulator-reopen-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let appended = database
        .append_simulator_turn(&chat_id, "hello", "ack")
        .unwrap();
    let session_id = appended.session_id;
    let before = appended.messages;
    let usage_before: (i64, i64, i64) = database
        .connection()
        .query_row(
            "SELECT turn_count,item_count,fact_count FROM acp_session_usage WHERE session_id=?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let usage_after: (i64, i64, i64) = reopened
        .connection()
        .query_row(
            "SELECT turn_count,item_count,fact_count FROM acp_session_usage WHERE session_id=?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let after = reopened.messages(&session_id).unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(
        after
            .iter()
            .map(|message| (&message.id, &message.role, &message.text, message.sequence))
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|message| (&message.id, &message.role, &message.text, message.sequence))
            .collect::<Vec<_>>()
    );
    assert_eq!(usage_after, usage_before);
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn simulator_turn_rolls_back_both_items_when_interrupted() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session_id = session(&database);
    let chat_id = chat(&database, &session_id);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        database.append_simulator_turn_with_hook(&chat_id, "user", "assistant", || {
            panic!("injected interruption")
        })
    }));
    assert!(result.is_err());
    assert_eq!(database.messages(&session_id).unwrap().len(), 1);
}

#[test]
fn simulator_turn_waits_for_project_switch_and_restart_is_stable() {
    let base = std::env::temp_dir().join(format!("sage-turn-race-{}", Uuid::new_v4()));
    let root_a = base.join("a");
    let root_b = base.join("b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();
    let database_path = base.join("sage.db");
    let database = Database::open(&database_path).unwrap();
    let project_a = database.create_project_from_root(&root_a).unwrap();
    let welcome_session_a: String = database
        .connection()
        .query_row(
            "SELECT id FROM agent_sessions WHERE project_id=?1",
            [&project_a.id],
            |row| row.get(0),
        )
        .unwrap();
    let chat_a = chat(&database, &welcome_session_a);
    let project_b = database.create_project_from_root(&root_b).unwrap();
    let current_b = database.bootstrap_snapshot().unwrap();
    database
        .switch_project(&ProjectSwitchRequest {
            target_project_id: project_a.id.clone(),
            current_layout: layout(&current_b),
        })
        .unwrap();
    let switch = ProjectSwitchRequest {
        target_project_id: project_b.id.clone(),
        current_layout: layout(&database.bootstrap_snapshot().unwrap()),
    };
    let (inserted_tx, inserted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let turn_database = database.clone();
    let turn_chat = chat_a.clone();
    let turn = thread::spawn(move || {
        turn_database.append_simulator_turn_with_hook(&turn_chat, "user", "assistant", || {
            inserted_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        })
    });
    inserted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let switching_database = database.clone();
    let switching = thread::spawn(move || switching_database.switch_project(&switch));
    release_tx.send(()).unwrap();
    let session_a = turn.join().unwrap().unwrap().session_id;
    switching.join().unwrap().unwrap();
    drop(database);
    let reopened = Database::open(&database_path).unwrap();
    assert_eq!(reopened.messages(&session_a).unwrap().len(), 3);
    assert_eq!(reopened.active_project_id().unwrap(), Some(project_b.id));
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn inactive_simulator_session_is_rejected_before_any_item() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session_a = session(&database);
    let chat_a = chat(&database, &session_a);
    let _session_b = session(&database);
    let before: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM items WHERE session_id=?1",
            [&session_a],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        database
            .append_simulator_turn(&chat_a, "user", "assistant")
            .is_err()
    );
    let after: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM items WHERE session_id=?1",
            [&session_a],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn transcript_uses_latest_bounded_window() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session_id = session(&database);
    let chat_id = chat(&database, &session_id);
    let mut connection = database.connection();
    let transaction = connection.transaction().unwrap();
    for index in 0..260 {
        let payloads = [
            payload("user", &format!("user-{index}")).unwrap(),
            payload("assistant", &format!("assistant-{index}")).unwrap(),
        ];
        store::append_turn(
            &transaction,
            &chat_id,
            crate::harnesses::SIMULATOR_DEFAULT_MODEL_ID,
            &payloads,
            index,
            || {},
        )
        .unwrap();
    }
    transaction.commit().unwrap();
    drop(connection);
    let messages = database.messages(&session_id).unwrap();
    assert_eq!(messages.len(), MAX_BOOTSTRAP_MESSAGES as usize);
    assert_eq!(messages.last().unwrap().text, "assistant-259");
    assert!(messages.first().unwrap().sequence > 1);
}

#[test]
fn native_projection_truncates_oldest_messages_without_rejecting_durable_turns() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let welcome_session_id = session(&database);
    let chat_id = chat(&database, &welcome_session_id);
    let large = "x".repeat(240_000);
    let mut last = None;
    for _ in 0..20 {
        last = Some(
            database
                .append_simulator_turn(&chat_id, &large, "ack")
                .unwrap(),
        );
    }
    let last = last.unwrap();
    let durable_items: i64 = database
        .connection()
        .query_row(
            "SELECT item_count FROM acp_session_usage WHERE session_id=?1",
            [&last.session_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(durable_items, 40);
    assert!(last.messages.len() < 41);
    assert_eq!(last.messages.last().unwrap().sequence, 41);
    assert!(
        last.messages
            .iter()
            .map(|message| message.id.len() + message.session_id.len() + message.text.len())
            .sum::<usize>()
            <= MAX_BOOTSTRAP_MESSAGE_BYTES as usize
    );
}

#[test]
fn transcript_rejects_a_mismatched_item_ledger_before_projection() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session_id = session(&database);
    database
        .connection()
        .execute(
            "INSERT INTO items(
               id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,
               payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at
             ) SELECT 'foreign-item',project_id,chat_id,session_id,turn_id,runtime_epoch,99,
                      origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,revision,
                      created_at,updated_at,terminal_at FROM items WHERE session_id=?1 LIMIT 1",
            [&session_id],
        )
        .unwrap();
    assert!(database.messages(&session_id).is_err());
}

#[test]
fn oversized_message_is_rejected_without_a_partial_turn() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session_id = session(&database);
    let chat_id = chat(&database, &session_id);
    let before: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM turns WHERE session_id=?1",
            [&session_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        database
            .append_simulator_turn(&chat_id, &"x".repeat(262_144), "assistant")
            .is_err()
    );
    let after: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM turns WHERE session_id=?1",
            [&session_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, before);
}

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

fn assert_ledgers_exact(connection: &rusqlite::Connection) {
    let exact: bool = connection
        .query_row(
            "SELECT
               NOT EXISTS(SELECT 1 FROM acp_session_usage u WHERE
                 u.turn_count<>(SELECT COUNT(*) FROM turns t WHERE t.session_id=u.session_id) OR
                 u.item_count<>(SELECT COUNT(*) FROM items i WHERE i.session_id=u.session_id) OR
                 u.fact_count<>(SELECT COUNT(*) FROM session_facts f WHERE f.session_id=u.session_id) OR
                 u.payload_bytes<>u.item_bytes+u.fact_bytes)
               AND NOT EXISTS(SELECT 1 FROM acp_project_usage p WHERE
                 p.chat_count<>(SELECT COUNT(*) FROM chats c WHERE c.project_id=p.project_id) OR
                 p.session_count<>(SELECT COUNT(*) FROM agent_sessions s WHERE s.project_id=p.project_id) OR
                 p.turn_count<>(SELECT COUNT(*) FROM turns t WHERE t.project_id=p.project_id) OR
                 p.item_count<>(SELECT COUNT(*) FROM items i WHERE i.project_id=p.project_id) OR
                 p.fact_count<>(SELECT COUNT(*) FROM session_facts f WHERE f.project_id=p.project_id))
               AND (SELECT turn_count FROM acp_store_usage WHERE singleton=1)=(SELECT COUNT(*) FROM turns)
               AND (SELECT item_count FROM acp_store_usage WHERE singleton=1)=(SELECT COUNT(*) FROM items)
               AND (SELECT fact_count FROM acp_store_usage WHERE singleton=1)=(SELECT COUNT(*) FROM session_facts)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(exact);
}

fn durable_chat_state(database: &Database, chat_id: &str) -> Vec<i64> {
    database
        .connection()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM agent_sessions WHERE chat_id=?1),
               (SELECT COUNT(*) FROM chat_session_heads WHERE chat_id=?1),
               (SELECT COUNT(*) FROM session_runtime_epochs WHERE chat_id=?1),
               (SELECT COUNT(*) FROM turns WHERE chat_id=?1),
               (SELECT COUNT(*) FROM items WHERE chat_id=?1),
               (SELECT COUNT(*) FROM session_facts WHERE chat_id=?1),
               (SELECT projection_revision FROM chats WHERE id=?1),
               (SELECT updated_at FROM chats WHERE id=?1),
               COALESCE((SELECT SUM(turn_count) FROM acp_session_usage WHERE chat_id=?1),0),
               COALESCE((SELECT SUM(item_count) FROM acp_session_usage WHERE chat_id=?1),0),
               COALESCE((SELECT SUM(item_bytes) FROM acp_session_usage WHERE chat_id=?1),0),
               COALESCE((SELECT SUM(fact_count) FROM acp_session_usage WHERE chat_id=?1),0),
               COALESCE((SELECT SUM(fact_bytes) FROM acp_session_usage WHERE chat_id=?1),0),
               COALESCE((SELECT SUM(payload_bytes) FROM acp_session_usage WHERE chat_id=?1),0),
               COALESCE((SELECT SUM(revision) FROM acp_session_usage WHERE chat_id=?1),0),
               (SELECT session_count FROM acp_project_usage p JOIN chats c ON c.project_id=p.project_id WHERE c.id=?1),
               (SELECT turn_count FROM acp_project_usage p JOIN chats c ON c.project_id=p.project_id WHERE c.id=?1),
               (SELECT item_count FROM acp_project_usage p JOIN chats c ON c.project_id=p.project_id WHERE c.id=?1),
               (SELECT fact_count FROM acp_project_usage p JOIN chats c ON c.project_id=p.project_id WHERE c.id=?1),
               (SELECT payload_bytes FROM acp_project_usage p JOIN chats c ON c.project_id=p.project_id WHERE c.id=?1),
               (SELECT revision FROM acp_project_usage p JOIN chats c ON c.project_id=p.project_id WHERE c.id=?1),
               (SELECT session_count FROM acp_store_usage WHERE singleton=1),
               (SELECT turn_count FROM acp_store_usage WHERE singleton=1),
               (SELECT item_count FROM acp_store_usage WHERE singleton=1),
               (SELECT fact_count FROM acp_store_usage WHERE singleton=1),
               (SELECT item_bytes FROM acp_store_usage WHERE singleton=1),
               (SELECT fact_bytes FROM acp_store_usage WHERE singleton=1),
               (SELECT shared_payload_bytes FROM acp_store_usage WHERE singleton=1),
               (SELECT revision FROM acp_store_usage WHERE singleton=1)",
            [chat_id],
            |row| (0..29).map(|index| row.get(index)).collect(),
        )
        .unwrap()
}
