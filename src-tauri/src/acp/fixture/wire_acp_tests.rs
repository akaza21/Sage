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

fn journey(id: &str) -> Vec<Value> {
    let case = super::super::generated_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("missing {id}"));
    serde_json::from_str(case.steps_json).unwrap()
}

fn replay(id: &str, requests: &[Value]) -> Vec<Value> {
    let mut state = WireState::new();
    let mut output = Vec::new();
    let lines: String = requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect();
    let mut input = Cursor::new(lines.into_bytes());
    for step in journey(id) {
        execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new()).unwrap();
    }
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn the_claude_config_journey_resumes_in_the_defaults_and_answers_each_change() {
    let session = super::acp::SESSION;
    let frames = replay(
        "claude-config-resume",
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"session/resume","params":{"sessionId":session}}),
            json!({"jsonrpc":"2.0","id":3,"method":"session/set_config_option","params":{"sessionId":session,"configId":"model","value":"fixture-model-deep"}}),
            json!({"jsonrpc":"2.0","id":4,"method":"session/set_config_option","params":{"sessionId":session,"configId":"effort","value":"max"}}),
            json!({"jsonrpc":"2.0","id":5,"method":"session/set_mode","params":{"sessionId":session,"modeId":"plan"}}),
            json!({"jsonrpc":"2.0","id":6,"method":"session/prompt","params":{"sessionId":session}}),
            json!({"jsonrpc":"2.0","id":"fixture-permission-1","result":{"outcome":{"outcome":"selected","optionId":"fixture-allow-once"}}}),
            json!({"jsonrpc":"2.0","id":7,"method":"session/close","params":{"sessionId":session}}),
        ],
    );
    let reply = |id: u64| frames.iter().find(|frame| frame["id"] == id).unwrap();
    let initialize = &reply(1)["result"];
    assert!(initialize["agentCapabilities"]["sessionCapabilities"]["resume"].is_object());
    assert_eq!(initialize["models"].as_array().unwrap().len(), 2);
    // A resumed session reports the adapter's defaults again.
    let resumed = &reply(2)["result"];
    assert_eq!(resumed["sessionId"], session);
    assert_eq!(resumed["modes"]["currentModeId"], "default");
    assert_eq!(resumed["configOptions"][0]["currentValue"], "fixture-model");
    assert_eq!(resumed["configOptions"][1]["currentValue"], "medium");
    assert_eq!(
        reply(3)["result"]["configOptions"][0]["currentValue"],
        "fixture-model-deep"
    );
    assert_eq!(reply(4)["error"]["code"], -32602);
    let mode = frames
        .iter()
        .position(|frame| frame["params"]["update"]["sessionUpdate"] == "current_mode_update")
        .unwrap();
    assert_eq!(frames[mode]["params"]["update"]["currentModeId"], "plan");
    assert_eq!(frames[mode + 1]["id"], 5);
    assert!(
        frames
            .iter()
            .any(|frame| frame["method"] == "session/request_permission")
    );
    assert_eq!(reply(6)["result"]["stopReason"], "end_turn");
}

#[test]
fn the_claude_config_journey_also_opens_a_new_session_and_refuses_a_foreign_resume() {
    let frames = replay(
        "claude-config-resume",
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{}}),
            json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{}}),
            json!({"jsonrpc":"2.0","id":"fixture-permission-1","result":{"outcome":{"outcome":"cancelled"}}}),
        ],
    );
    assert_eq!(frames[1]["result"]["modes"]["currentModeId"], "default");
    let mut state = WireState::new();
    let foreign = format!(
        "{}\n",
        json!({"jsonrpc":"2.0","id":2,"method":"session/resume","params":{"sessionId":"other"}})
    );
    assert!(
        execute_with_io(
            &json!({"op":"awaitClient","kind":"acpOpen"}),
            &mut state,
            &mut Cursor::new(foreign.into_bytes()),
            &mut Vec::new(),
            &mut Vec::new()
        )
        .is_err()
    );
}

#[test]
fn the_codex_config_journey_starts_or_resumes_the_same_thread() {
    for open in ["thread/start", "thread/resume"] {
        let frames = replay(
            "codex-config-resume",
            &[
                json!({"id":1,"method":"initialize","params":{}}),
                json!({"id":2,"method":open,"params":{}}),
                json!({"id":3,"method":"model/list","params":{"cursor":null}}),
                json!({"id":4,"method":"thread/goal/get","params":{}}),
                json!({"id":5,"method":"turn/start","params":{}}),
                json!({"id":6,"method":"thread/unsubscribe","params":{}}),
            ],
        );
        let thread = frames.iter().find(|frame| frame["id"] == 2).unwrap();
        assert_eq!(thread["result"]["thread"]["id"], super::acp::SESSION);
        let models = frames.iter().find(|frame| frame["id"] == 3).unwrap();
        assert_eq!(models["result"]["data"].as_array().unwrap().len(), 2);
    }
}
