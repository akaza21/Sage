use super::*;
use crate::acp::runtime::DriverMailbox;

fn second_chat(db: &Database, first: &PumpIntent) -> PumpIntent {
    let connection = db.connection();
    // Copy the completed exchange into an independent Chat in the same store.
    let replacements = [
        (first.chat_id.as_str(), "chat-two"),
        (first.session_id.as_str(), "session-two"),
        (first.active_turn.turn_id.as_str(), "turn-two"),
        ("title-tab", "title-tab-two"),
    ];
    for (table, filter) in [
        ("chats", "id='chat-one'"),
        ("workbench_tabs", "id='title-tab'"),
        ("chat_tabs", "chat_id='chat-one'"),
        ("agent_sessions", "chat_id='chat-one'"),
        ("chat_session_heads", "chat_id='chat-one'"),
        ("session_runtime_epochs", "chat_id='chat-one'"),
        ("turns", "chat_id='chat-one'"),
        ("items", "chat_id='chat-one'"),
        ("acp_session_usage", "chat_id='chat-one'"),
    ] {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} WHERE {filter}"))
            .unwrap();
        let columns = statement
            .column_names()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let rows = statement
            .query_map([], |row| {
                (0..columns.len())
                    .map(|i| row.get::<_, rusqlite::types::Value>(i))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for mut row in rows {
            for (column, value) in columns.iter().zip(&mut row) {
                if (table == "chats" && column == "ordinal")
                    || (table == "workbench_tabs" && column == "position")
                {
                    *value = rusqlite::types::Value::Integer(1);
                }
                if let rusqlite::types::Value::Text(text) = value {
                    if let Some((_, replacement)) =
                        replacements.iter().find(|(original, _)| text == *original)
                    {
                        *text = replacement.to_string();
                    } else if table == "items" && column == "id" {
                        *text = format!("other-{text}");
                    }
                }
            }
            let placeholders = vec!["?"; columns.len()].join(",");
            connection
                .execute(
                    &format!("INSERT INTO {table} VALUES({placeholders})"),
                    rusqlite::params_from_iter(row),
                )
                .unwrap();
        }
    }
    let mut second = first.clone();
    second.chat_id = sage_acp_protocol::contract::SageChatId::parse("chat-two").unwrap();
    second.session_id =
        sage_acp_protocol::contract::SageAgentSessionId::parse("session-two").unwrap();
    second.active_turn = crate::acp::runtime::ActiveTurnIntent::store_minted(
        sage_acp_protocol::contract::SageTurnId::parse("turn-two").unwrap(),
    )
    .unwrap();
    second
}

#[test]
fn another_chat_finishing_wakes_a_pending_title_without_a_new_turn() {
    for preempted in [false, true] {
        let (db, a) = crate::acp::store::title_fixture();
        let b = second_chat(&db, &a);
        let admission = Arc::new(TitleAdmission::default());
        let mut titles_a = scheduler(&db, &admission);
        let mut titles_b = scheduler(&db, &admission);
        let mailbox_a = Arc::new(DriverMailbox::new().unwrap());
        let mailbox_b = Arc::new(DriverMailbox::new().unwrap());
        titles_a.bind_wakeup(owner(&a), &mailbox_a);
        titles_b.bind_wakeup(owner(&b), &mailbox_b);
        let mut live_a = runtime();
        let mut live_b = runtime();
        if preempted {
            titles_a.completed(&a, &mut live_a);
        }
        let foreground_b = titles_b.foreground_turn();
        if preempted {
            finish(&mut titles_a, &mut live_a);
        } else {
            titles_a.completed(&a, &mut live_a);
        }
        assert!(!titles_a.pending());
        titles_b.completed(&b, &mut live_b);
        drop(foreground_b);
        // The oldest pending marker wins; B cannot steal A's wakeup.
        titles_b.poll(&mut live_b);
        assert!(!titles_b.pending());
        finish(&mut titles_a, &mut live_a);
        assert_eq!(
            db.connection()
                .query_row("SELECT title FROM chats WHERE id='chat-one'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "Chat: Investigate empty sidebar chats",
            "preempted={preempted}"
        );
        finish(&mut titles_b, &mut live_b);
        assert_eq!(
            db.connection()
                .query_row("SELECT title FROM chats WHERE id='chat-two'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "Chat: Investigate empty sidebar chats"
        );
        assert_eq!(
            db.connection()
                .query_row("SELECT count(*) FROM turns", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}

fn owner(intent: &PumpIntent) -> sage_acp_protocol::scheduler::RuntimeOwner {
    sage_acp_protocol::scheduler::RuntimeOwner {
        project_id: intent.project_id.clone(),
        chat_id: intent.chat_id.clone(),
        session_id: intent.session_id.clone(),
        runtime_epoch: intent.runtime_epoch,
    }
}

#[test]
fn admission_released_before_the_pending_marker_commits_still_wakes_the_title() {
    let (db, a) = crate::acp::store::title_fixture();
    let admission = Arc::new(TitleAdmission::default());
    let mut titles = scheduler(&db, &admission);
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    titles.bind_wakeup(owner(&a), &mailbox);
    let mut live = runtime();
    // The first idle poll has already happened; no activation wake can mask
    // the race between admission and durable publication of the marker.
    titles.poll(&mut live);
    let foreground = titles.foreground_turn();
    let busy = admission.acquire(&owner(&a));
    assert!(busy.is_none());
    drop(foreground);
    titles.record_completion(owner(&a), a.active_turn.turn_id.clone(), busy, &mut live);
    finish(&mut titles, &mut live);
    assert_eq!(shown(&db), "Chat: Investigate empty sidebar chats");
}

#[test]
fn releasing_the_title_lease_wakes_the_busy_chat_without_a_new_completion() {
    let (db, a) = crate::acp::store::title_fixture();
    let b = second_chat(&db, &a);
    let admission = Arc::new(TitleAdmission::default());
    let mut titles_a = scheduler(&db, &admission);
    let mut titles_b = scheduler(&db, &admission);
    let mailbox_a = Arc::new(DriverMailbox::new().unwrap());
    let mailbox_b = Arc::new(DriverMailbox::new().unwrap());
    titles_a.bind_wakeup(owner(&a), &mailbox_a);
    titles_b.bind_wakeup(owner(&b), &mailbox_b);
    let mut live_a = runtime();
    let mut live_b = runtime();
    titles_b.completed(&b, &mut live_b);
    titles_a.completed(&a, &mut live_a);
    assert!(!titles_a.pending());
    finish(&mut titles_b, &mut live_b);
    finish(&mut titles_a, &mut live_a);
    assert_eq!(
        db.connection()
            .query_row("SELECT title FROM chats WHERE id='chat-one'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "Chat: Investigate empty sidebar chats"
    );
    assert_eq!(
        db.connection()
            .query_row("SELECT count(*) FROM turns", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn a_closed_mailbox_retained_for_recovery_cannot_hold_the_next_pending_chat() {
    let (db, a) = crate::acp::store::title_fixture();
    let b = second_chat(&db, &a);
    let admission = Arc::new(TitleAdmission::default());
    let mut titles_a = scheduler(&db, &admission);
    let mut titles_b = scheduler(&db, &admission);
    let mailbox_a = Arc::new(DriverMailbox::new().unwrap());
    let mailbox_b = Arc::new(DriverMailbox::new().unwrap());
    titles_a.bind_wakeup(owner(&a), &mailbox_a);
    titles_b.bind_wakeup(owner(&b), &mailbox_b);
    let mut live_a = runtime();
    let mut live_b = runtime();
    let foreground = titles_b.foreground_turn();
    titles_a.completed(&a, &mut live_a);
    titles_b.completed(&b, &mut live_b);
    drop(foreground);
    // A was selected before close. Recovery retains its scheduler and mailbox.
    mailbox_a.close();
    finish(&mut titles_b, &mut live_b);
    assert_eq!(
        db.connection()
            .query_row("SELECT title FROM chats WHERE id='chat-two'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "Chat: Investigate empty sidebar chats"
    );
    assert_eq!(
        db.connection()
            .query_row("SELECT title FROM chats WHERE id='chat-one'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "Chat: Prompt fallback"
    );
    assert!(!titles_a.pending());
}

#[test]
fn pending_title_wakes_after_park_and_reactivation_without_another_turn() {
    use crate::acp::store::{
        self, ActivateSessionCommand, CompleteActivationCommand, ParkSessionCommand,
    };
    use sage_acp_protocol::contract::{
        ClientIntentId, ProjectionRevision, RequiredText, RuntimeEpoch,
    };
    let (db, a) = crate::acp::store::title_fixture();
    let b = second_chat(&db, &a);
    let admission = Arc::new(TitleAdmission::default());
    let mut titles_a = scheduler(&db, &admission);
    let mut titles_b = scheduler(&db, &admission);
    let mailbox_a = Arc::new(DriverMailbox::new().unwrap());
    let mailbox_b = Arc::new(DriverMailbox::new().unwrap());
    titles_a.bind_wakeup(owner(&a), &mailbox_a);
    titles_b.bind_wakeup(owner(&b), &mailbox_b);
    let mut live_a = runtime();
    let mut live_b = runtime();
    let foreground_b = titles_b.foreground_turn();
    titles_a.completed(&a, &mut live_a);
    drop(titles_a);
    let epoch = RuntimeEpoch::new(2).unwrap();
    {
        let mut connection = db.write_connection().unwrap();
        store::park_session_after_zero_process(
            &mut connection,
            ParkSessionCommand {
                project_id: a.project_id.clone(),
                chat_id: a.chat_id.clone(),
                session_id: a.session_id.clone(),
                runtime_epoch: a.runtime_epoch,
                exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
                now_millis: 10,
            },
        )
        .unwrap();
        let revision: i64 = connection
            .query_row(
                "SELECT projection_revision FROM chats WHERE id='chat-one'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        store::activate_session(
            &mut connection,
            ActivateSessionCommand {
                project_id: a.project_id.clone(),
                chat_id: a.chat_id.clone(),
                session_id: a.session_id.clone(),
                runtime_epoch: epoch,
                expected_projection_revision: ProjectionRevision::new(revision as u64).unwrap(),
                client_intent_id: ClientIntentId::parse("title-reactivate").unwrap(),
                artifact_digest: "d".repeat(64),
                now_millis: 11,
            },
        )
        .unwrap();
        let (binding_digest, capabilities_json): (String, String) = connection.query_row("SELECT binding_digest,capabilities_json FROM agent_sessions WHERE id='session-one'", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        store::complete_activation(
            &mut connection,
            CompleteActivationCommand {
                project_id: a.project_id.clone(),
                chat_id: a.chat_id.clone(),
                session_id: a.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest,
                artifact_digest: "d".repeat(64),
                external_session_id: a.external_session_id.clone(),
                capabilities_json,
                now_millis: 12,
            },
        )
        .unwrap();
    }
    let mut reopened = scheduler(&db, &admission);
    let mut resumed_owner = owner(&a);
    resumed_owner.runtime_epoch = epoch;
    reopened.bind_wakeup(resumed_owner, &mailbox_a);
    titles_b.completed(&b, &mut live_b);
    drop(foreground_b);
    finish(&mut reopened, &mut live_a);
    assert_eq!(
        db.connection()
            .query_row("SELECT title FROM chats WHERE id='chat-one'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "Chat: Investigate empty sidebar chats"
    );
    assert_eq!(
        db.connection()
            .query_row(
                "SELECT count(*) FROM turns WHERE chat_id='chat-one'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    finish(&mut titles_b, &mut live_b);
}
