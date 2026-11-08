use super::*;
use crate::acp::{
    intent::session_event::{SessionActivityState, SessionScopedEvent},
    lifecycle::ToolState,
};

fn hook_started(turn: Value) -> Frame {
    frame(json!({"method":"hook/started","params":{
        "threadId":"thread-1","turnId":turn,
        "run":{
            "id":"session-start:0:/home/dev/.codex/hooks.json","eventName":"sessionStart",
            "handlerType":"command","executionMode":"sync","scope":"thread",
            "sourcePath":"/home/dev/.codex/hooks.json","displayOrder":0,"status":"running",
            "statusMessage":null,"startedAt":1_700_000_000,"completedAt":null,
            "durationMs":null,"entries":[]
        }
    }}))
}

#[test]
fn a_turnless_hook_reaches_the_pump_as_an_exact_session_event() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let batch = runtime
        .ingest(&hook_started(Value::Null), &Mapper)
        .expect("hook admitted");
    let [AdapterEvent::Session { id, event }] = batch.events() else {
        panic!("expected one session event, got {:?}", batch.events())
    };
    assert_eq!(id.external_id.as_str(), "thread-1");
    assert_eq!(id.turn_id, None);
    assert_eq!(id.turn_client_message_id, None);
    assert!(matches!(
        event,
        SessionScopedEvent::Activity(activity)
            if activity.kind.as_str() == "hook"
                && activity.label.as_str() == "SessionStart hook"
                && activity.state == SessionActivityState::Running
                && activity.progress.is_none()
    ));
}

#[test]
fn mcp_progress_reaches_the_pump_as_an_update_of_the_running_item() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let started = runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1",
                "item":{"type":"mcpToolCall","id":"mcp-1","server":"docs","tool":"search",
                    "status":"inProgress","arguments":{},"result":null,"error":null}
            }})),
            &Mapper,
        )
        .expect("item started");
    let [AdapterEvent::Item { id: started_id, .. }] = started.events() else {
        panic!("expected the started item, got {:?}", started.events())
    };
    let progress = runtime
        .ingest(
            &frame(json!({"method":"item/mcpToolCall/progress","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"mcp-1",
                "message":"Fetched 2 of 4 pages"
            }})),
            &Mapper,
        )
        .expect("progress admitted");
    let [
        AdapterEvent::Item {
            id,
            lifecycle: ItemLifecycle::Streaming,
            tool_state: Some(ToolState::InProgress),
            payload: NormalizedPayload::ToolCall { execution, .. },
        },
    ] = progress.events()
    else {
        panic!("expected one item update, got {:?}", progress.events())
    };
    assert_eq!(id, started_id, "progress updates the existing item");
    assert_eq!(
        execution.progress.as_ref().map(|line| line.as_str()),
        Some("Fetched 2 of 4 pages")
    );
    assert_eq!(
        execution.mcp_server.as_ref().map(|server| server.as_str()),
        Some("docs")
    );
}
