use super::*;
use serde_json::json;
use std::io::Cursor;

#[test]
fn codex_child_read_is_correlated_without_consuming_parent_request() {
    let mut state = WireState::new();
    let mut output = Vec::new();
    let requests = br#"{"id":91,"method":"thread/read","params":{"threadId":"fixture-child-1","includeTurns":true}}
{"id":92,"method":"thread/unsubscribe"}
"#;
    execute_with_io(
        &json!({"op":"awaitClient","kind":"codexUnsubscribe"}),
        &mut state,
        &mut Cursor::new(requests),
        &mut output,
        &mut Vec::new(),
    )
    .unwrap();
    execute_with_io(
        &json!({"op":"emitKnownFrame","shape":"codexAck"}),
        &mut state,
        &mut Cursor::new([]),
        &mut output,
        &mut Vec::new(),
    )
    .unwrap();
    let frames: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames[0]["id"], 91);
    assert_eq!(frames[0]["result"]["thread"]["id"], "fixture-child-1");
    assert_eq!(
        frames[0]["result"]["thread"]["parentThreadId"],
        "fixture-persisted-session-1"
    );
    assert_eq!(
        frames[0]["result"]["thread"]["turns"][0]["itemsView"],
        "full"
    );
    assert_eq!(frames[1]["id"], 92);
}

#[test]
fn codex_child_read_rejects_foreign_identity_and_bounds_waits() {
    for request in [
        json!({"id":1,"method":"thread/read","params":{"threadId":"foreign","includeTurns":true}}),
        json!({"id":1,"method":"thread/read","params":{"threadId":"fixture-child-1","includeTurns":false}}),
    ] {
        let bytes = format!("{request}\n");
        assert!(
            execute_with_io(
                &json!({"op":"awaitClient","kind":"codexUnsubscribe"}),
                &mut WireState::new(),
                &mut Cursor::new(bytes),
                &mut Vec::new(),
                &mut Vec::new()
            )
            .is_err()
        );
    }
    let bytes = concat!(r#"{"method":"initialized"}"#, "\n").repeat(33);
    assert!(
        execute_with_io(
            &json!({"op":"awaitClient","kind":"codexUnsubscribe"}),
            &mut WireState::new(),
            &mut Cursor::new(bytes),
            &mut Vec::new(),
            &mut Vec::new()
        )
        .is_err()
    );
}
