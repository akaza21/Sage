use super::*;
use crate::acp::session_action::SessionAction;

fn arm_action(adapter: &mut CodexAdapter, request_id: u64, action: Value) -> String {
    let mut prompt = Prompt::new("sessionAction", "/action").unwrap();
    prompt.action = Some(serde_json::from_value::<SessionAction>(action).unwrap());
    let client = RequiredText::new("client", "client-action").unwrap();
    let request = adapter.prompt(request_id, &prompt, &client).unwrap();
    let value: Value = serde_json::from_slice(&request.bytes).unwrap();
    value["method"].as_str().unwrap().to_owned()
}

fn events(adapter: &mut CodexAdapter, frame: Value) -> Vec<IncomingEvent> {
    match adapter
        .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
        .unwrap()
    {
        IngressDisposition::Events(events) => events,
        other => panic!("expected events, got {other:?}"),
    }
}

/// Turn states and item payloads as Sage projects them, each scoped to the
/// action's client message.
fn projected(events: &[IncomingEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            IncomingEvent::Turn { id, state } => {
                assert_eq!(
                    id.turn_client_message_id.as_ref().unwrap().as_str(),
                    "client-action"
                );
                format!("turn {state:?}")
            }
            IncomingEvent::Item { id, payload, .. } => {
                assert_eq!(
                    id.turn_client_message_id.as_ref().unwrap().as_str(),
                    "client-action"
                );
                match payload {
                    NormalizedPayload::Text { text } => format!("text {}", text.as_str()),
                    NormalizedPayload::ToolCall { label, .. } => format!("tool {}", label.as_str()),
                    NormalizedPayload::CompactionNotice => "compaction".to_owned(),
                    other => panic!("unexpected payload {other:?}"),
                }
            }
            other => panic!("unexpected event {other:?}"),
        })
        .collect()
}

fn notification(method: &str, params: Value) -> Value {
    json!({"method": method, "params": params})
}

fn review_item(kind: &str, review: &str) -> Value {
    json!({"threadId": "thread-1", "turnId": "turn-review",
        "item": {"type": kind, "id": "turn-review", "review": review}})
}

#[test]
fn an_inline_review_projects_as_an_ordinary_turn_ending_in_its_findings() {
    let mut adapter = adapter();
    ready(&mut adapter);
    let target = json!({"kind": "review", "target": {"kind": "uncommittedChanges"}});
    assert_eq!(arm_action(&mut adapter, 60, target), "review/start");
    let mut sequence = events(
        &mut adapter,
        json!({"id": 60, "result": {"reviewThreadId": "thread-1", "turn": {
            "id": "turn-review", "status": "inProgress", "itemsView": "notLoaded",
            "items": [{"type": "userMessage", "id": "turn-review", "clientId": null,
                "content": [{"type": "text", "text": "current changes", "text_elements": []}]}]
        }}}),
    );
    for frame in [
        notification(
            "turn/started",
            json!({"threadId": "thread-1", "turn": {"id": "turn-review", "status": "inProgress"}}),
        ),
        notification(
            "item/started",
            review_item("enteredReviewMode", "current changes"),
        ),
        notification(
            "item/completed",
            review_item("exitedReviewMode", "No issues found."),
        ),
        notification(
            "turn/completed",
            json!({"threadId": "thread-1", "turn": {"id": "turn-review", "status": "completed"}}),
        ),
    ] {
        sequence.extend(events(&mut adapter, frame));
    }
    assert_eq!(
        projected(&sequence),
        [
            "turn Sending",
            "turn Running",
            "tool Entered review mode",
            "text No issues found.",
            "turn Completed",
        ]
    );
    assert!(!adapter.has_active_turn() && !adapter.has_pending_turn_start());
}

#[test]
fn a_compaction_binds_the_turn_that_follows_its_empty_response() {
    let mut adapter = adapter();
    ready(&mut adapter);
    assert_eq!(
        arm_action(&mut adapter, 61, json!({"kind": "compact"})),
        "thread/compact/start"
    );
    assert_eq!(
        adapter
            .ingest(br#"{"id":61,"result":{}}"#, &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
    let compaction = json!({"threadId": "thread-1", "turnId": "turn-compact",
        "item": {"type": "contextCompaction", "id": "compact-1"}});
    let mut sequence = Vec::new();
    for frame in [
        notification(
            "turn/started",
            json!({"threadId": "thread-1", "turn": {"id": "turn-compact", "status": "inProgress"}}),
        ),
        notification("item/completed", compaction),
        notification(
            "turn/completed",
            json!({"threadId": "thread-1", "turn": {"id": "turn-compact", "status": "completed"}}),
        ),
    ] {
        sequence.extend(events(&mut adapter, frame));
    }
    assert_eq!(
        projected(&sequence),
        ["turn Running", "compaction", "turn Completed"]
    );
    assert!(!adapter.has_pending_turn_start());
    arm_turn(&mut adapter, 62, "client-next");
}
