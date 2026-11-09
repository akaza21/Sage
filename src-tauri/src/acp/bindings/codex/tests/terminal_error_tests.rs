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

fn error_notification(message: &str, will_retry: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({"method":"error","params":{
        "threadId":"thread-1","turnId":"turn-1","willRetry":will_retry,
        "error":{"message":message}
    }}))
    .unwrap()
}

#[test]
fn full_unretried_error_ledger_keeps_accepting_errors_and_preserves_completion() {
    for recorded_terminal_error in [true, false] {
        let mut adapter = running_adapter();
        for index in 0..crate::acp::limits::LIMITS.wire.items_per_turn {
            let message = format!("Fatal cause {index}");
            assert!(matches!(
                adapter.ingest(&error_notification(&message, false), &Mapper).unwrap(),
                IngressDisposition::Events(events)
                    if matches!(events.as_slice(), [IncomingEvent::Error(_)])
            ));
        }
        assert!(matches!(
            adapter.ingest(&error_notification("Overflow fatal cause", false), &Mapper).unwrap(),
            IngressDisposition::Events(events)
                if matches!(events.as_slice(), [IncomingEvent::Error(_)])
        ));
        let message = if recorded_terminal_error {
            "Fatal cause 0"
        } else {
            "Overflow fatal cause"
        };
        let IngressDisposition::Events(events) = adapter
            .ingest(&completion(json!({"message":message})), &Mapper)
            .unwrap()
        else {
            panic!()
        };
        if recorded_terminal_error {
            assert!(matches!(
                events.as_slice(),
                [IncomingEvent::Turn {
                    state: TurnState::Failed,
                    ..
                }]
            ));
        } else {
            assert!(matches!(events.as_slice(), [
                IncomingEvent::Error(NormalizedPayload::Error { message, retryable: false, .. }),
                IncomingEvent::Turn { state: TurnState::Failed, .. }
            ] if message.as_str() == "Overflow fatal cause"));
        }
    }
}

#[test]
fn unretried_error_notification_suppresses_the_matching_terminal_error() {
    let mut adapter = running_adapter();
    let message = "Failed with sk-private at C:\\private\\file.txt";
    let IngressDisposition::Events(events) = adapter
        .ingest(&error_notification(message, false), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(events.as_slice(), [
        IncomingEvent::Error(NormalizedPayload::Error { message, retryable: false, .. })
    ] if message.as_str() == "Failed with [redacted] at [path]"));
    let frame = completion(json!({"message":message}));
    let IngressDisposition::Events(events) = adapter.ingest(&frame, &Mapper).unwrap() else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Turn {
            state: TurnState::Failed,
            ..
        }]
    ));
    assert_eq!(
        adapter.ingest(&frame, &Mapper).unwrap(),
        IngressDisposition::DuplicateTerminal
    );
}

#[test]
fn retryable_or_distinct_error_notification_preserves_the_terminal_error() {
    for (earlier, terminal, will_retry) in [
        ("Fatal cause", "Fatal cause", true),
        ("Earlier cause", "Fatal cause", false),
        (
            "Failed at C:\\private\\one.txt",
            "Failed at C:\\private\\two.txt",
            false,
        ),
    ] {
        let mut adapter = running_adapter();
        adapter
            .ingest(&error_notification(earlier, will_retry), &Mapper)
            .unwrap();
        let IngressDisposition::Events(events) = adapter
            .ingest(&completion(json!({"message":terminal})), &Mapper)
            .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            events.as_slice(),
            [
                IncomingEvent::Error(NormalizedPayload::Error {
                    retryable: false,
                    ..
                }),
                IncomingEvent::Turn {
                    state: TurnState::Failed,
                    ..
                }
            ]
        ));
    }
}

#[test]
fn terminal_error_matches_any_unretried_error_in_the_turn_not_only_the_last() {
    let mut adapter = running_adapter();
    for message in ["First fatal cause", "Second fatal cause"] {
        adapter
            .ingest(&error_notification(message, false), &Mapper)
            .unwrap();
    }
    let IngressDisposition::Events(events) = adapter
        .ingest(&completion(json!({"message":"First fatal cause"})), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Turn {
            state: TurnState::Failed,
            ..
        }]
    ));
}

#[test]
fn previous_turn_error_does_not_suppress_a_new_turns_terminal_error() {
    let mut adapter = running_adapter();
    adapter
        .ingest(br#"{"id":2,"result":{"turn":{"id":"turn-1"}}}"#, &Mapper)
        .unwrap();
    adapter
        .ingest(&error_notification("Fatal cause", false), &Mapper)
        .unwrap();
    adapter.ingest(&completion(Value::Null), &Mapper).unwrap();
    arm_turn(&mut adapter, 3, "client-next-terminal-error");
    adapter
        .ingest(br#"{"id":3,"result":{"turn":{"id":"turn-2"}}}"#, &Mapper)
        .unwrap();
    let IngressDisposition::Events(events) = adapter.ingest(
        br#"{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-2","status":"failed","error":{"message":"Fatal cause"}}}}"#,
        &Mapper,
    ).unwrap() else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [
            IncomingEvent::Error(NormalizedPayload::Error {
                retryable: false,
                ..
            }),
            IncomingEvent::Turn {
                state: TurnState::Failed,
                ..
            }
        ]
    ));
}

#[test]
fn terminal_only_error_is_sanitized_before_the_failed_turn_and_duplicate_is_absorbed() {
    let mut adapter = running_adapter();
    let frame = completion(json!({"message":"Failed with sk-private at C:\\private\\file.txt"}));
    let IngressDisposition::Events(events) = adapter.ingest(&frame, &Mapper).unwrap() else {
        panic!()
    };
    assert!(matches!(events.as_slice(), [
        IncomingEvent::Error(NormalizedPayload::Error { code, message, retryable: false, .. }),
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

fn interrupted() -> Vec<u8> {
    serde_json::to_vec(&json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}
    }}))
    .unwrap()
}

#[test]
fn an_interrupt_sage_never_wrote_leaves_the_codex_stop_interrupted() {
    let mut adapter = running_adapter();
    adapter.cancel(3).unwrap();
    adapter.reject_before_write(3).unwrap();
    let IngressDisposition::Events(events) = adapter.ingest(&interrupted(), &Mapper).unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Turn {
            state: TurnState::Interrupted,
            ..
        }]
    ));
}

#[test]
fn a_user_stop_leaves_the_plan_open_for_the_turn_sweep() {
    let mut adapter = running_adapter();
    let plan = json!({"method":"turn/plan/updated","params":{
        "threadId":"thread-1","turnId":"turn-1","explanation":null,
        "plan":[{"step":"Inspect","status":"inProgress"}]
    }});
    adapter
        .ingest(&serde_json::to_vec(&plan).unwrap(), &Mapper)
        .unwrap();
    adapter.cancel(3).unwrap();
    let IngressDisposition::Events(events) = adapter.ingest(&interrupted(), &Mapper).unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Turn {
            state: TurnState::Cancelled,
            ..
        }]
    ));
}
