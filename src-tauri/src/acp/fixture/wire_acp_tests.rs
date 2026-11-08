use super::*;
use serde_json::json;
use std::io::Cursor;

#[path = "wire_native_projection_tests.rs"]
mod native_projection;

#[test]
fn replay_resource_paths_resolve_existing_files_in_the_capture_project() {
    let root = std::env::temp_dir().join(format!("sage-replay-path-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/example.ts"), b"export const example = 1;\n").unwrap();
    let canonical = root.canonicalize().unwrap();
    let value = super::acp::substitute_root("$ROOT/src/example.ts", &canonical).unwrap();
    assert_eq!(
        std::path::Path::new(&value).canonicalize().unwrap(),
        root.join("src/example.ts").canonicalize().unwrap()
    );
    #[cfg(windows)]
    assert!(!value.contains('/'));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn acp_replay_preserves_prompt_id_across_config_and_cancel() {
    let mut state = WireState::new();
    let mut output = Vec::new();
    let mut error = Vec::new();
    let requests = b"{\"jsonrpc\":\"2.0\",\"id\":41,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"session/new\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":43,\"method\":\"session/prompt\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":44,\"method\":\"session/set_mode\",\"params\":{\"modeId\":\"default\"}}\n{\"jsonrpc\":\"2.0\",\"method\":\"session/cancel\",\"params\":{}}\n";
    let mut input = Cursor::new(requests);
    for step in [
        json!({"op":"awaitClient","kind":"acpInitialize"}),
        json!({"op":"emitKnownFrame","shape":"acpInitialize"}),
        json!({"op":"awaitClient","kind":"acpNew"}),
        json!({"op":"emitKnownFrame","shape":"acpSession"}),
        json!({"op":"awaitClient","kind":"acpPrompt"}),
        json!({"op":"awaitClient","kind":"acpCancel"}),
        json!({"op":"emitKnownFrame","shape":"acpCancelled"}),
    ] {
        execute_with_io(&step, &mut state, &mut input, &mut output, &mut error).unwrap();
    }
    let frames: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames[0]["id"], 41);
    assert_eq!(frames[1]["id"], 42);
    assert_eq!(frames[2]["id"], 44);
    assert_eq!(
        frames[3],
        json!({"jsonrpc":"2.0","id":43,"result":{"stopReason":"cancelled"}})
    );
}

#[test]
fn acp_replay_rejects_unmatched_permission_and_wrong_method() {
    let mut state = WireState::new();
    for bytes in [
        b"{\"id\":1,\"method\":\"session/prompt\"}\n".as_slice(),
        b"{\"id\":\"foreign-permission\",\"result\":{}}\n".as_slice(),
    ] {
        assert!(
            execute_with_io(
                &json!({"op":"awaitClient","kind":"acpPermissionDecision"}),
                &mut state,
                &mut Cursor::new(bytes),
                &mut Vec::new(),
                &mut Vec::new()
            )
            .is_err()
        );
    }
}

#[test]
fn acp_prompt_text_must_match_the_single_text_block_exactly() {
    let step = json!({"op":"awaitClient","kind":"acpPrompt","promptText":"/review 42"});
    for (prompt, accepted) in [
        (json!([{"type":"text","text":"/review 42"}]), true),
        (json!([{"type":"text","text":"/review 42 "}]), false),
        (json!([{"type":"text","text":"/compact"}]), false),
        (
            json!([{"type":"text","text":"/review 42"},{"type":"text","text":"more"}]),
            false,
        ),
    ] {
        let mut state = WireState::new();
        let request = json!({"jsonrpc":"2.0","id":7,"method":"session/prompt","params":{"sessionId":"s","prompt":prompt}});
        let mut input = Cursor::new(format!("{request}\n").into_bytes());
        let result = execute_with_io(
            &step,
            &mut state,
            &mut input,
            &mut Vec::new(),
            &mut Vec::new(),
        );
        assert_eq!(result.is_ok(), accepted, "{prompt}");
    }
    let wrong_kind = json!({"op":"awaitClient","kind":"acpNew","promptText":"/compact"});
    let mut input =
        Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session/new\"}\n".to_vec());
    assert!(
        execute_with_io(
            &wrong_kind,
            &mut WireState::new(),
            &mut input,
            &mut Vec::new(),
            &mut Vec::new()
        )
        .is_err()
    );
}

#[test]
fn claude_slash_commands_case_advertises_three_commands_and_checks_each_prompt() {
    let prompt = |id: u64, text: &str| json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{"sessionId":"fixture-persisted-session-1","prompt":[{"type":"text","text":text}]}});
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{}}),
        prompt(3, "/review 42"),
        prompt(4, "/compact"),
        prompt(5, "Summarize the example"),
        json!({"jsonrpc":"2.0","id":6,"method":"session/close","params":{}}),
    ]
    .iter()
    .map(|request| format!("{request}\n"))
    .collect::<String>();
    let frames = super::capture::replay_native("claude-slash-commands", &requests);
    let update = &frames[2]["params"]["update"];
    assert_eq!(update["sessionUpdate"], "available_commands_update");
    let commands = update["availableCommands"].as_array().unwrap();
    let names: Vec<_> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["compact", "review", "init"]);
    assert_eq!(commands[1]["input"]["hint"], "PR number");
    let finished: Vec<_> = frames
        .iter()
        .filter(|frame| frame["result"]["stopReason"] == "end_turn")
        .map(|frame| frame["id"].as_u64().unwrap())
        .collect();
    assert_eq!(finished, [3, 4, 5]);

    // A prompt in the wrong order, or freehand text, is refused.
    let case = super::super::generated_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == "claude-slash-commands")
        .unwrap();
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let swapped = requests.replacen("/review 42", "/compact", 1);
    let mut input = Cursor::new(swapped.into_bytes());
    let mut state = WireState::new();
    let outcome = steps.iter().try_for_each(|step| {
        execute_with_io(
            step,
            &mut state,
            &mut input,
            &mut Vec::new(),
            &mut Vec::new(),
        )
    });
    assert!(outcome.is_err());
}
