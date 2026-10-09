use super::*;
use serde_json::json;
use std::io::Cursor;

#[test]
fn codex_model_list_returns_one_correlated_terminal_catalog_page() {
    let mut state = WireState::new();
    let mut input = Cursor::new(concat!(
        r#"{"id":71,"method":"model/list","params":{"cursor":null}}"#,
        "\n"
    ));
    let mut output = Vec::new();
    for step in [
        json!({"op":"awaitClient","kind":"codexModelList"}),
        json!({"op":"emitKnownFrame","shape":"codexModelListResult"}),
    ] {
        execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new()).unwrap();
    }
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["id"], 71);
    assert_eq!(reply["result"]["nextCursor"], Value::Null);
    assert_eq!(reply["result"]["data"].as_array().unwrap().len(), 1);
    assert_eq!(reply["result"]["data"][0]["id"], "fixture-model");
    assert!(output.len() < 1_024);
}

#[test]
fn codex_optional_model_hydration_preserves_goal_and_turn_correlation() {
    for models in [false, true] {
        let mut bytes = String::new();
        if models {
            bytes.push_str("{\"id\":71,\"method\":\"model/list\",\"params\":{\"cursor\":null}}\n");
        }
        bytes.push_str(concat!(
            r#"{"id":72,"method":"thread/goal/get","params":{}}"#,
            "\n",
            r#"{"id":73,"method":"turn/start","params":{}}"#,
            "\n"
        ));
        let mut input = Cursor::new(bytes);
        let mut state = WireState::new();
        let mut output = Vec::new();
        for step in [
            json!({"op":"awaitClient","kind":"codexGoalGetWithOptionalModelList"}),
            json!({"op":"emitKnownFrame","shape":"codexGoalMethodNotFound"}),
            json!({"op":"awaitClient","kind":"codexTurnStart"}),
            json!({"op":"emitKnownFrame","shape":"codexTurnResult"}),
        ] {
            execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new()).unwrap();
        }
        let frames: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(frames.len(), if models { 3 } else { 2 });
        let goal = usize::from(models);
        if models {
            assert_eq!(frames[0]["id"], 71);
            assert_eq!(frames[0]["result"]["nextCursor"], Value::Null);
        }
        assert_eq!(frames[goal]["id"], 72);
        assert_eq!(frames[goal]["error"]["code"], -32601);
        assert_eq!(frames[goal + 1]["id"], 73);
        assert_eq!(state.turn_count, 1);
    }
}

#[test]
fn codex_model_hydration_is_scoped_single_page_and_strict() {
    let model = json!({"id":71,"method":"model/list","params":{"cursor":null}});
    for (kind, requests) in [
        ("codexTurnStart", vec![model.clone()]),
        ("codexGoalGet", vec![model.clone()]),
        (
            "codexGoalGetWithOptionalModelList",
            vec![model.clone(), model],
        ),
        (
            "codexGoalGetWithOptionalModelList",
            vec![json!({"id":72,"method":"account/read"})],
        ),
        (
            "codexModelList",
            vec![json!({"id":73,"method":"model/list","params":{"cursor":"next"}})],
        ),
        (
            "codexGoalGetWithOptionalModelList",
            vec![json!({"id":74,"method":"model/list","params":{"cursor":"next"}})],
        ),
    ] {
        let bytes = requests
            .iter()
            .map(|request| format!("{request}\n"))
            .collect::<String>();
        assert!(
            execute_with_io(
                &json!({"op":"awaitClient","kind":kind}),
                &mut WireState::new(),
                &mut Cursor::new(bytes),
                &mut Vec::new(),
                &mut Vec::new()
            )
            .is_err(),
            "{kind}"
        );
    }
}

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
