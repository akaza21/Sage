use super::*;
use crate::acp::{projection::ItemContent, store::load_chat_page};
use sage_acp_protocol::lifecycle::{ItemLifecycle, TurnState, TurnTerminalReason};

struct TempRoot(std::path::PathBuf);
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) struct Reopened {
    pub(super) database: sage_db::database::Database,
    _root: TempRoot,
}
impl Reopened {
    pub(super) fn new(fixture: &Fixture) -> Self {
        let root = std::env::temp_dir().join(format!("sage-pi-turn-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("sage.db");
        {
            let source = fixture.database.connection();
            let mut copy = rusqlite::Connection::open(&path).unwrap();
            rusqlite::backup::Backup::new(&source, &mut copy)
                .unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None)
                .unwrap();
        }
        Self {
            database: crate::database::open_database(&path).unwrap(),
            _root: TempRoot(root),
        }
    }
}

#[tokio::test]
async fn emitted_pi_failure_cause_is_sanitized_bounded_and_durable_after_reopen() {
    for (detail, expected) in [
        (
            Some("Rate limited".to_owned()),
            Some("Rate limited".to_owned()),
        ),
        (None, None),
        (Some(" \n\t".to_owned()), None),
        (
            Some(
                "Denied sk-private Bearer private C:\\secret\\file /home/private token=private"
                    .to_owned(),
            ),
            Some("Denied [redacted] Bearer [redacted] [path] [path] [redacted]".to_owned()),
        ),
        (Some("é".repeat(600)), Some(format!("{}…", "é".repeat(511)))),
    ] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        let mut message = json!({"role":"assistant","stopReason":"error"});
        if let Some(detail) = detail {
            message["errorMessage"] = json!(detail);
        }
        pump.push_stdout_chunk(&line(json!({"type":"agent_end","messages":[message]})), 3)
            .await
            .unwrap();
        assert!(!pump.completed_turn_ready());
        pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 4)
            .await
            .unwrap();
        assert!(pump.completed_turn_ready());
        let reopened = Reopened::new(&fixture);
        for database in [&fixture.database, &reopened.database] {
            let connection = database.connection();
            let page =
                load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
            assert_eq!(page.turns.as_slice().len(), 1);
            let turn = &page.turns.as_slice()[0];
            assert_eq!(turn.state, TurnState::Failed);
            assert_eq!(
                turn.terminal_reason.as_ref().unwrap().kind,
                TurnTerminalReason::Other
            );
            assert_eq!(
                turn.terminal_reason
                    .as_ref()
                    .and_then(|reason| reason.message.as_ref())
                    .map(|message| message.as_str()),
                expected.as_deref()
            );
        }
        drop(reopened);
        fixture.finish().await;
    }
}

async fn attempt(
    pump: &mut Pump,
    text: &str,
    stop: &str,
    cause: Option<&str>,
    before_end: Option<&Value>,
) {
    for event in [
        json!({"type":"message_start","message":{"role":"assistant"}}),
        json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":text}}),
    ] {
        pump.push_stdout_chunk(&line(event), 3).await.unwrap();
    }
    if let Some(event) = before_end {
        pump.push_stdout_chunk(&line(event.clone()), 3)
            .await
            .unwrap();
    }
    let mut message = json!({"role":"assistant","stopReason":stop});
    if let Some(cause) = cause {
        message["errorMessage"] = json!(cause);
    }
    for event in [
        json!({"type":"message_end","message":message}),
        json!({"type":"agent_end","messages":[{"role":"assistant","stopReason":stop}]}),
    ] {
        pump.push_stdout_chunk(&line(event), 3).await.unwrap();
        assert!(!pump.completed_turn_ready());
    }
}

async fn retained_retry(
    outcome: TurnState,
    cause: Option<&str>,
    early_end: bool,
    streaming_cancel: bool,
) {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    attempt(
        &mut pump,
        "Failed attempt retained",
        "error",
        Some("Attempt unavailable"),
        None,
    )
    .await;
    let retry = json!({"type":"auto_retry_start","attempt":1,"maxAttempts":3,"delayMs":1,"errorMessage":"Server unavailable"});
    for _ in 0..2 {
        pump.push_stdout_chunk(&line(retry.clone()), 4)
            .await
            .unwrap();
    }
    assert!(!pump.completed_turn_ready());
    let mut retry_end =
        json!({"type":"auto_retry_end","attempt":1,"success":outcome == TurnState::Completed});
    if let Some(cause) = cause {
        retry_end["finalError"] = json!(cause);
    }
    if outcome == TurnState::Cancelled {
        if streaming_cancel {
            for event in [
                json!({"type":"agent_start"}),
                json!({"type":"message_start","message":{"role":"assistant"}}),
                json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Second attempt retained"}}),
            ] {
                pump.push_stdout_chunk(&line(event), 5).await.unwrap();
                assert!(!pump.completed_turn_ready());
            }
        }
        let intent = fixture.intent(0);
        {
            let mut connection = fixture.database.connection();
            let page =
                load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
            crate::acp::store::begin_cancel(
                &mut connection,
                crate::acp::store::BeginCancelCommand {
                    turn: crate::acp::store::TurnMutationCommand {
                        project_id: intent.project_id.clone(),
                        chat_id: intent.chat_id.clone(),
                        session_id: intent.session_id.clone(),
                        runtime_epoch: intent.runtime_epoch,
                        turn_id: intent.active_turn.turn_id.clone(),
                        expected_revision: page.turns.as_slice()[0].revision,
                        now_millis: 5,
                    },
                    client_intent_id: sage_acp_protocol::contract::ClientIntentId::parse(
                        "cancel-pi-retry",
                    )
                    .unwrap(),
                },
            )
            .unwrap();
        }
        pump.queue_cancel_notification(
            &intent.session_id,
            intent.runtime_epoch,
            &intent.active_turn.turn_id,
            &intent.active_turn.client_message_id,
        )
        .unwrap();
        let mut capture = Capture::default();
        pump.flush_one(&mut capture).unwrap();
        assert_eq!(capture.0[0]["type"], "abort");
        pump.push_stdout_chunk(&reply(&capture.0[0], Value::Null), 5)
            .await
            .unwrap();
        if streaming_cancel {
            pump.push_stdout_chunk(&line(json!({"type":"agent_end","messages":[{"role":"assistant","stopReason":"aborted"}]})), 6).await.unwrap();
        }
    } else {
        pump.push_stdout_chunk(&line(json!({"type":"agent_start"})), 5)
            .await
            .unwrap();
        attempt(
            &mut pump,
            "Second attempt retained",
            if outcome == TurnState::Completed {
                "stop"
            } else {
                "error"
            },
            early_end.then_some("Attempt unavailable"),
            early_end.then_some(&retry_end),
        )
        .await;
    }
    if !early_end {
        pump.push_stdout_chunk(&line(retry_end), 6).await.unwrap();
    }
    assert!(!pump.completed_turn_ready());
    pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 7)
        .await
        .unwrap();
    assert!(pump.completed_turn_ready());
    let reopened = Reopened::new(&fixture);
    for database in [&fixture.database, &reopened.database] {
        let connection = database.connection();
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        assert_eq!(page.turns.as_slice().len(), 1);
        let turn = &page.turns.as_slice()[0];
        assert_eq!(turn.id.as_str(), "turn-pump-0");
        assert_eq!(turn.state, outcome);
        let texts = turn
            .items
            .as_slice()
            .iter()
            .filter_map(|item| match &item.content {
                ItemContent::Text { text, .. } => Some((item.lifecycle, text.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(texts[0], (ItemLifecycle::Failed, "Failed attempt retained"));
        if outcome == TurnState::Cancelled && !streaming_cancel {
            assert_eq!(texts.len(), 1);
        } else {
            assert_eq!(texts.len(), 2);
            assert_eq!(
                texts[1],
                (
                    match outcome {
                        TurnState::Completed => ItemLifecycle::Completed,
                        TurnState::Cancelled => ItemLifecycle::Cancelled,
                        _ => ItemLifecycle::Failed,
                    },
                    "Second attempt retained"
                )
            );
        }
        let errors = turn
            .items
            .as_slice()
            .iter()
            .filter_map(|item| match &item.content {
                ItemContent::Error { message, .. } => Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(errors, ["Server unavailable"]);
        assert_eq!(
            turn.terminal_reason
                .as_ref()
                .and_then(|reason| reason.message.as_deref()),
            if outcome == TurnState::Failed {
                cause
            } else {
                None
            }
        );
        match outcome {
            TurnState::Completed => assert!(turn.terminal_reason.is_none()),
            TurnState::Failed => assert_eq!(
                turn.terminal_reason.as_ref().unwrap().kind,
                TurnTerminalReason::Other
            ),
            TurnState::Cancelled => assert_eq!(
                turn.terminal_reason.as_ref().unwrap().kind,
                TurnTerminalReason::CancelledByUser
            ),
            _ => unreachable!(),
        }
    }
    drop(reopened);
    fixture.finish().await;
}

#[tokio::test]
async fn automatic_retry_success_retains_failed_attempt_in_one_turn_after_sqlite_reopen() {
    retained_retry(TurnState::Completed, None, false, false).await;
}

#[tokio::test]
async fn automatic_retry_exhaustion_retains_attempts_and_final_cause_after_sqlite_reopen() {
    retained_retry(TurnState::Failed, Some("Retries exhausted"), false, false).await;
}

#[tokio::test]
async fn automatic_retry_cancel_during_backoff_retains_failed_attempt_after_sqlite_reopen() {
    retained_retry(TurnState::Cancelled, None, false, false).await;
}

#[tokio::test]
async fn exhausted_retry_final_cause_precedes_later_attempt_error_after_sqlite_reopen() {
    retained_retry(TurnState::Failed, Some("Retries exhausted"), true, false).await;
}

#[tokio::test]
async fn exhausted_retry_without_final_cause_does_not_reuse_earlier_cause_after_sqlite_reopen() {
    retained_retry(TurnState::Failed, None, false, false).await;
}

#[tokio::test]
async fn automatic_retry_cancel_during_streaming_retains_both_attempts_after_sqlite_reopen() {
    retained_retry(TurnState::Cancelled, None, false, true).await;
}

#[tokio::test]
async fn explicit_retry_creates_a_new_turn_and_preserves_prior_failure_after_sqlite_reopen() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    attempt(
        &mut pump,
        "Original failed attempt",
        "error",
        Some("Attempt unavailable"),
        None,
    )
    .await;
    pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 4)
        .await
        .unwrap();
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("retry turn"));
    start_turn(&mut pump, 1).await;
    attempt(&mut pump, "Explicit retry answer", "stop", None, None).await;
    pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 5)
        .await
        .unwrap();
    let reopened = Reopened::new(&fixture);
    {
        let connection = reopened.database.connection();
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        assert_eq!(page.turns.as_slice().len(), 2);
        let original = page
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.id.as_str() == "turn-pump-0")
            .unwrap();
        assert_eq!(original.state, TurnState::Failed);
        assert_eq!(
            original
                .terminal_reason
                .as_ref()
                .unwrap()
                .message
                .as_deref(),
            Some("Attempt unavailable")
        );
        let retry = page
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.id.as_str() == "turn-pump-1")
            .unwrap();
        assert_eq!(retry.state, TurnState::Completed);
        assert!(retry.items.as_slice().iter().any(|item| matches!(&item.content, ItemContent::Text { text, .. } if text.as_str() == "Explicit retry answer")));
    }
    drop(reopened);
    fixture.finish().await;
}
