use super::*;

mod config;
mod content_free;
mod continuation;
mod dialog_boundaries;
mod dialogs;
mod discovery;
mod fixture;
mod journey;
mod lifecycle_history;
mod refresh_capacity;
mod review_nits;

use fixture::*;

#[derive(Clone, Copy)]
enum Row {
    Session,
    Config,
    Lifecycle,
    Content,
    ContentFree,
    FireAndForget,
    Unknown,
}
#[derive(Clone, Copy)]
enum State {
    Idle,
    Live,
    Closed,
}

fn row_frames(row: Row) -> Vec<Value> {
    match row {
        Row::Session => [
            "thinking_level_changed",
            "session_info_changed",
            "queue_update",
            "entry_appended",
        ]
        .into_iter()
        .map(|kind| json!({"type":kind,"level":"high"}))
        .collect(),
        Row::Lifecycle => [
            "turn_start",
            "turn_end",
            "auto_retry_start",
            "auto_retry_end",
            "summarization_retry_start",
            "summarization_retry_end",
        ]
        .into_iter()
        .map(|kind| json!({"type":kind}))
        .collect(),
        Row::Content => vec![
            json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Hello"}}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","delta":"Inspecting"}}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"thinking_end"}}),
            json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]}}),
            tool_start("read-content"),
            json!({"type":"tool_execution_update","toolCallId":"read-content","partialResult":{"content":[{"type":"text","text":"Reading"}]}}),
            tool_end(
                "read-content",
                json!([{"type":"text","text":"Read"},{"type":"image","mimeType":"image/png","data":"aGVsbG8="}]),
            ),
            json!({"type":"compaction_start"}),
            json!({"type":"compaction_end","result":{}}),
        ],
        Row::ContentFree => content_free::updates(),
        Row::FireAndForget => content_free::notifications(),
        Row::Unknown => [
            "future_event",
            "future_event",
            "another_event",
            "another_event",
        ]
        .into_iter()
        .map(|kind| json!({"type":kind}))
        .collect(),
        Row::Config => unreachable!(),
    }
}

async fn exercise(row: Row, state: State) {
    if matches!(row, Row::Config) {
        config::exercise(state).await;
        return;
    }
    if matches!(state, State::Idle) {
        for reset in [false, true] {
            let mut activation = activation_idle(reset);
            for value in row_frames(row) {
                activation.accept_idle_stdout_chunk(&line(value)).unwrap();
            }
        }
    }
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    if !matches!(state, State::Live) {
        settle(&mut pump).await;
    }
    fixture.observed.lock().unwrap().clear();
    if matches!(state, State::Idle) {
        let mut idle = pump
            .try_into_completed_parts()
            .unwrap_or_else(|_| panic!("idle handoff"));
        for value in row_frames(row) {
            idle.accept_idle_stdout_chunk(&line(value), 10, &mut Capture::default())
                .await
                .unwrap();
        }
    } else {
        for value in row_frames(row) {
            pump.push_stdout_chunk(&line(value), 10).await.unwrap();
        }
        assert!(!pump.teardown_required());
    }
    let events = fixture.observed.lock().unwrap().clone();
    match (row, state) {
        (Row::Session, _) | (_, State::Idle | State::Closed) => {
            assert!(!events.is_empty());
            assert!(
                events
                    .iter()
                    .all(|event| matches!(event, AdapterEvent::IgnoredLate))
            );
        }
        (Row::Lifecycle | Row::ContentFree | Row::FireAndForget, State::Live) => {
            assert_eq!(events.len(), row_frames(row).len());
            assert!(
                events
                    .iter()
                    .all(|event| matches!(event, AdapterEvent::Absorbed))
            );
            assert_eq!(fixture.item_count(), 0);
        }
        (Row::Unknown, State::Live) => {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, AdapterEvent::Unsupported(_)))
                    .count(),
                2
            );
            assert_eq!(fixture.item_count(), 2);
        }
        (Row::Content, State::Live) => {
            for kind in [
                "text",
                "thoughtSummary",
                "toolCall",
                "compactionNotice",
                "unsupported",
            ] {
                assert!(events.iter().any(|event| matches!(event, AdapterEvent::Item { payload, .. } if payload.kind().as_str() == kind)), "missing {kind}");
            }
            assert_eq!(fixture.item_count(), 5);
        }
        (Row::Config, State::Live) => unreachable!(),
    }
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_session_notifications_idle() {
    exercise(Row::Session, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_session_notifications_live() {
    exercise(Row::Session, State::Live).await;
}
#[tokio::test]
async fn pi_pump_session_notifications_closed() {
    exercise(Row::Session, State::Closed).await;
}
#[tokio::test]
async fn pi_pump_config_replies_idle() {
    exercise(Row::Config, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_config_replies_live() {
    exercise(Row::Config, State::Live).await;
}
#[tokio::test]
async fn pi_pump_config_replies_closed() {
    exercise(Row::Config, State::Closed).await;
}
#[tokio::test]
async fn pi_pump_lifecycle_markers_idle() {
    exercise(Row::Lifecycle, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_lifecycle_markers_live() {
    exercise(Row::Lifecycle, State::Live).await;
}
#[tokio::test]
async fn pi_pump_lifecycle_markers_closed() {
    exercise(Row::Lifecycle, State::Closed).await;
}
#[tokio::test]
async fn pi_pump_content_events_idle() {
    exercise(Row::Content, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_content_events_live() {
    exercise(Row::Content, State::Live).await;
}
#[tokio::test]
async fn pi_pump_content_events_closed() {
    exercise(Row::Content, State::Closed).await;
}
#[tokio::test]
async fn pi_pump_unknown_events_idle() {
    exercise(Row::Unknown, State::Idle).await;
}
#[tokio::test]
async fn pi_pump_unknown_events_live() {
    exercise(Row::Unknown, State::Live).await;
}
#[tokio::test]
async fn pi_pump_unknown_events_closed() {
    exercise(Row::Unknown, State::Closed).await;
}

#[tokio::test]
async fn pi_pump_authority_denied_drops_all_contract_rows() {
    let fixture = Fixture::new();
    let mut adapter = ready_adapter();
    let config = config::pending_replies(&mut adapter.inner);
    let mut pump = fixture.pump(adapter);
    start(&mut pump).await;
    pump.push_stdout_chunk(&line(json!({"type":"tool_execution_start","toolCallId":"write-denied","toolName":"write","args":{}})), 5).await.unwrap();
    fixture.observed.lock().unwrap().clear();
    for row in [
        Row::Session,
        Row::Lifecycle,
        Row::Content,
        Row::ContentFree,
        Row::FireAndForget,
        Row::Unknown,
    ] {
        for value in row_frames(row) {
            pump.push_stdout_chunk(&line(value), 6).await.unwrap();
        }
    }
    for value in config {
        pump.push_stdout_chunk(&value, 6).await.unwrap();
    }
    assert!(
        fixture
            .observed
            .lock()
            .unwrap()
            .iter()
            .all(|event| matches!(event, AdapterEvent::IgnoredLate))
    );
    assert!(!pump.teardown_required());
    fixture.finish().await;
}
