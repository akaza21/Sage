use super::*;

fn running_adapter() -> CodexAdapter {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-terminal-error");
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
            }}))
            .unwrap(),
            &Mapper,
        )
        .unwrap();
    adapter
}

fn completion(error: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"failed","error":error}
    }}))
    .unwrap()
}

#[test]
fn terminal_only_error_is_sanitized_before_the_failed_turn_and_duplicate_is_absorbed() {
    let mut adapter = running_adapter();
    let frame = completion(json!({"message":"Failed with sk-private at C:\\private\\file.txt"}));
    let IngressDisposition::Events(events) = adapter.ingest(&frame, &Mapper).unwrap() else {
        panic!()
    };
    assert!(matches!(events.as_slice(), [
        IncomingEvent::Error(NormalizedPayload::Error { code, message, retryable: false }),
        IncomingEvent::Turn { state: TurnState::Failed, .. }
    ] if code.as_str() == "codex" && message.as_str() == "Failed with [redacted] at [path]"));
    assert_eq!(
        adapter.ingest(&frame, &Mapper).unwrap(),
        IngressDisposition::DuplicateTerminal
    );
}

#[test]
fn malformed_or_oversized_terminal_error_does_not_settle_the_turn() {
    for error in [
        json!(false),
        json!({}),
        json!({"message":false}),
        json!({"message":"x".repeat(crate::acp::limits::LIMITS.wire.error_bytes as usize + 1)}),
    ] {
        let mut adapter = running_adapter();
        assert!(adapter.ingest(&completion(error), &Mapper).is_err());
        assert!(
            matches!(adapter.ingest(&completion(Value::Null), &Mapper).unwrap(), IngressDisposition::Events(events) if matches!(events.as_slice(), [IncomingEvent::Turn { state: TurnState::Failed, .. }]))
        );
    }
}

#[test]
fn absent_or_null_terminal_error_does_not_invent_an_error_item() {
    for status in ["completed", "interrupted", "failed"] {
        for include_error in [false, true] {
            let mut adapter = running_adapter();
            let mut turn = json!({"id":"turn-1","status":status});
            if include_error {
                turn["error"] = Value::Null;
            }
            let frame = serde_json::to_vec(
                &json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":turn}}),
            )
            .unwrap();
            assert!(
                matches!(adapter.ingest(&frame, &Mapper).unwrap(), IngressDisposition::Events(events) if matches!(events.as_slice(), [IncomingEvent::Turn { .. }]))
            );
        }
    }
}
