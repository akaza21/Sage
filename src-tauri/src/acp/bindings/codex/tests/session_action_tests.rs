use super::*;
use crate::acp::session_action::SessionAction;

fn arm_action(adapter: &mut CodexAdapter, request_id: u64, action: Value) {
    let mut prompt = Prompt::new("sessionAction", "/action").unwrap();
    prompt.action = Some(serde_json::from_value::<SessionAction>(action).unwrap());
    let client = RequiredText::new("client", "client-action").unwrap();
    adapter.prompt(request_id, &prompt, &client).unwrap();
}

/// The live pinned Codex app-server frames of a capture under
/// `harness-traces/codex/`, with its thread renamed to the bound `thread-1`.
fn captured(trace: &str, thread: &str) -> Vec<(String, Value)> {
    trace
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let record: Value = serde_json::from_str(&line.replace(thread, "thread-1")).unwrap();
            (
                record["direction"].as_str().unwrap().to_owned(),
                record["frame"].clone(),
            )
        })
        .collect()
}

/// Replays every frame after the captured `thread/start` answer until
/// `thread/unsubscribe`, issuing the same requests through the adapter under
/// its own request ids.
fn replay(adapter: &mut CodexAdapter, frames: &[(String, Value)]) -> Vec<IncomingEvent> {
    let start = frames
        .iter()
        .position(|(_, frame)| frame["result"]["thread"]["id"] == "thread-1")
        .unwrap();
    let mut ids = Vec::new();
    let mut events = Vec::new();
    for (direction, frame) in &frames[start + 1..] {
        if direction == "out" {
            let ours = 100 + ids.len() as u64;
            match frame["method"].as_str().unwrap() {
                "turn/start" => arm_turn(adapter, ours, "client-prompt"),
                "review/start" => arm_action(
                    adapter,
                    ours,
                    json!({"kind": "review", "target": {"kind": "uncommittedChanges"}}),
                ),
                "thread/compact/start" => arm_action(adapter, ours, json!({"kind": "compact"})),
                "thread/unsubscribe" => break,
                other => panic!("unexpected captured request {other}"),
            }
            ids.push((frame["id"].clone(), ours));
            continue;
        }
        let mut frame = frame.clone();
        if frame.get("method").is_none()
            && let Some((_, ours)) = ids.iter().find(|(captured, _)| *captured == frame["id"])
        {
            frame["id"] = json!(ours);
        }
        match adapter
            .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
            .unwrap_or_else(|error| panic!("captured frame {frame} failed: {error:?}"))
        {
            IngressDisposition::Events(batch) => events.extend(batch),
            IngressDisposition::IgnoredLate => {}
            other => panic!("captured frame {frame} read as {other:?}"),
        }
    }
    events
}

/// Each Turn event and item update as Sage projects it, prefixed by the
/// client message its Turn is scoped to.
fn projected(events: &[IncomingEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| {
            let (id, row) = match event {
                IncomingEvent::Turn { id, state } => (id, format!("turn {state:?}")),
                IncomingEvent::Item {
                    id,
                    lifecycle,
                    payload,
                    ..
                } => (
                    id,
                    format!("{lifecycle:?} ")
                        + &match payload {
                            NormalizedPayload::Text { text } => format!("text {}", text.as_str()),
                            NormalizedPayload::ToolCall { label, .. } => {
                                format!("tool {}", label.as_str())
                            }
                            NormalizedPayload::CompactionNotice => "compaction".to_owned(),
                            other => panic!("unexpected payload {other:?}"),
                        },
                ),
                IncomingEvent::Error(NormalizedPayload::Error { message, .. }) => {
                    return Some(format!("error {}", message.as_str()));
                }
                _ => return None,
            };
            let client = id.turn_client_message_id.as_ref().unwrap().as_str();
            Some(format!("{client}: {row}"))
        })
        .collect()
}

const PROVIDER_REFUSAL: &str = r#"{"error": {"message": "fixture provider refuses every request", "type": "invalid_request_error", "code": "fixture"}}"#;

#[test]
fn the_captured_inline_review_is_one_sage_turn_named_by_its_root() {
    let frames = captured(
        include_str!(
            "../../../../../tests/fixtures/harness-traces/codex/review-inline/frames.jsonl"
        ),
        "01a1bbe6-7d8e-7c02-976b-784a01e72196",
    );
    let started = frames
        .iter()
        .find(|(_, frame)| frame["method"] == "turn/started")
        .map(|(_, frame)| &frame["params"]["turn"])
        .unwrap();
    // The live shape this test pins: the nested turn is not the one the
    // request answered, and only its root names the review Turn.
    assert_ne!(started["id"], started["rootTurnId"]);
    let mut adapter = adapter();
    ready(&mut adapter);
    let events = replay(&mut adapter, &frames);
    assert_eq!(
        projected(&events),
        [
            "client-action: turn Sending",
            "client-action: Started tool Entered review mode",
            "client-action: Completed tool Entered review mode",
            "client-action: turn Running",
            &format!("error {PROVIDER_REFUSAL}"),
            "client-action: Started text Reviewer failed to output a response.",
            "client-action: Completed text Reviewer failed to output a response.",
            // Sage redacts what reads as an absolute path in harness prose.
            "client-action: Started text Review was interrupted. Please re-run [path] and wait for it to complete.",
            "client-action: Completed text Review was interrupted. Please re-run [path] and wait for it to complete.",
            "client-action: turn Failed",
        ]
    );
    assert!(!adapter.has_active_turn() && !adapter.has_pending_turn_start());
    arm_turn(&mut adapter, 200, "client-next");
}

#[test]
fn the_captured_compaction_binds_the_turn_that_follows_its_empty_answer() {
    let frames = captured(
        include_str!("../../../../../tests/fixtures/harness-traces/codex/compact/frames.jsonl"),
        "01a1bbe6-af7d-7521-85df-2cc7fc445796",
    );
    let mut adapter = adapter();
    ready(&mut adapter);
    let events = replay(&mut adapter, &frames);
    assert_eq!(
        projected(&events),
        [
            "client-prompt: turn Sending",
            "client-prompt: turn Running",
            &format!("error {PROVIDER_REFUSAL}"),
            "client-prompt: turn Failed",
            "client-action: turn Running",
            "client-action: Completed compaction",
            &format!("error {PROVIDER_REFUSAL}"),
            "client-action: turn Failed",
        ]
    );
    assert!(!adapter.has_action_handoff() && !adapter.has_pending_turn_start());
    arm_turn(&mut adapter, 200, "client-next");
}

fn system_error() -> Vec<u8> {
    serde_json::to_vec(&json!({"method": "thread/status/changed",
        "params": {"threadId": "thread-1", "status": {"type": "systemError"}}}))
    .unwrap()
}

#[test]
fn a_system_error_before_the_compaction_turn_is_named_fails_that_turn() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_action(&mut adapter, 61, json!({"kind": "compact"}));
    assert_eq!(
        adapter
            .ingest(br#"{"id":61,"result":{}}"#, &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
    assert!(adapter.has_action_handoff() && !adapter.provisional_expired());
    let IngressDisposition::Events(events) = adapter.ingest(&system_error(), &Mapper).unwrap()
    else {
        panic!("a system error ends the held compaction");
    };
    assert_eq!(
        projected(&events),
        [
            "error Codex reported a system error while starting the turn. Check the selected model or harness connection, then retry.",
            "client-action: turn Failed",
        ]
    );
    assert!(!adapter.has_action_handoff() && !adapter.has_pending_turn_start());
    arm_turn(&mut adapter, 62, "client-next");
}

#[test]
fn a_cancel_before_the_compaction_turn_is_named_waits_for_that_turn() {
    let mut adapter = adapter();
    ready(&mut adapter);
    assert!(!adapter.defer_cancel());
    arm_action(&mut adapter, 61, json!({"kind": "compact"}));
    adapter
        .ingest(br#"{"id":61,"result":{}}"#, &Mapper)
        .unwrap();
    assert!(adapter.defer_cancel());
    assert!(!adapter.take_deferred_interrupt());
    adapter
        .ingest(
            br#"{"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-compact","rootTurnId":"turn-compact","status":"inProgress"}}}"#,
            &Mapper,
        )
        .unwrap();
    assert!(adapter.take_deferred_interrupt());
    assert!(!adapter.take_deferred_interrupt());
    let interrupt = adapter.cancel(62).unwrap();
    let value: Value = serde_json::from_slice(&interrupt.bytes).unwrap();
    assert_eq!(
        (value["method"].clone(), value["params"]["turnId"].clone()),
        (json!("turn/interrupt"), json!("turn-compact"))
    );
}

#[test]
fn a_held_cancel_is_dropped_when_its_turn_fails_before_it_is_named() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_action(&mut adapter, 61, json!({"kind": "compact"}));
    adapter
        .ingest(br#"{"id":61,"result":{}}"#, &Mapper)
        .unwrap();
    assert!(adapter.defer_cancel());
    adapter.ingest(&system_error(), &Mapper).unwrap();
    arm_turn(&mut adapter, 62, "client-next");
    adapter
        .ingest(
            br#"{"id":62,"result":{"turn":{"id":"turn-next","status":"inProgress"}}}"#,
            &Mapper,
        )
        .unwrap();
    assert!(!adapter.take_deferred_interrupt());
}
