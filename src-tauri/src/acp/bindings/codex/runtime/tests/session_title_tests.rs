use super::*;
use crate::acp::intent::session_event::SessionScopedEvent;

fn name_update(
    runtime: &mut CodexRuntime,
    params: Value,
) -> Result<AdapterBatch, AdapterRuntimeError> {
    runtime.ingest(
        &frame(json!({ "method": "thread/name/updated", "params": params })),
        &Mapper,
    )
}

fn session_title(batch: &AdapterBatch) -> Option<String> {
    match batch.events() {
        [
            AdapterEvent::Session {
                id,
                event: SessionScopedEvent::Title { title: Some(title) },
            },
        ] => {
            assert_eq!(id.external_id.as_str(), "thread-1");
            assert_eq!(id.turn_id, None);
            assert_eq!(id.turn_client_message_id, None);
            Some(title.as_str().to_owned())
        }
        [AdapterEvent::IgnoredLate] => None,
        other => panic!("unexpected events {other:?}"),
    }
}

#[test]
fn thread_name_becomes_a_session_title_without_a_turn() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let batch = name_update(
        &mut runtime,
        json!({ "threadId": "thread-1", "threadName": "  Fix the login redirect\nmore" }),
    )
    .unwrap();
    assert_eq!(
        session_title(&batch).as_deref(),
        Some("Fix the login redirect")
    );
}

#[test]
fn a_cleared_or_blank_thread_name_keeps_the_shown_title() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    for params in [
        json!({ "threadId": "thread-1" }),
        json!({ "threadId": "thread-1", "threadName": null }),
        json!({ "threadId": "thread-1", "threadName": " \u{7} " }),
    ] {
        let batch = name_update(&mut runtime, params).unwrap();
        assert_eq!(session_title(&batch), None);
    }
}

#[test]
fn malformed_or_foreign_thread_names_are_rejected() {
    for params in [
        json!({ "threadId": "thread-1", "threadName": 7 }),
        json!({ "threadId": "thread-1", "threadName": "x", "extra": true }),
        json!({ "threadName": "x" }),
        json!({ "threadId": "other-thread", "threadName": "x" }),
    ] {
        let mut runtime = fixture_runtime();
        ready(&mut runtime);
        assert!(
            name_update(&mut runtime, params.clone()).is_err(),
            "{params}"
        );
    }
}
