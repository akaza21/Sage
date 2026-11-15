use serde_json::{Value, json};

use super::capture::replay_native;
use sage_core::limits::LIMITS;

fn lines(requests: &[Value]) -> String {
    requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect()
}

fn codex_turn(turn: Value) -> String {
    lines(&[
        json!({"id":1,"method":"initialize","params":{}}),
        json!({"id":2,"method":"thread/resume","params":{"threadId":"fixture-persisted-session-1","model":"fixture-model-deep"}}),
        json!({"id":3,"method":"model/list","params":{}}),
        json!({"id":4,"method":"thread/goal/get","params":{}}),
        json!({"id":5,"method":"turn/start","params":turn}),
        json!({"id":6,"method":"thread/unsubscribe","params":{}}),
    ])
}

fn codex_answer(frames: &[Value]) -> &str {
    frames
        .iter()
        .find(|frame| frame["method"] == "item/completed")
        .and_then(|frame| frame["params"]["item"]["text"].as_str())
        .expect("the turn answers")
}

#[test]
fn the_codex_config_echo_names_the_model_and_effort_turn_start_sent() {
    let picked = replay_native(
        "codex-config-echo",
        &codex_turn(json!({"model":"fixture-model-deep","effort":"high","input":[]})),
    );
    assert_eq!(
        codex_answer(&picked),
        "This turn/start named model: fixture-model-deep; effort: high."
    );
    let literal = replay_native(
        "codex-config-echo",
        &codex_turn(json!({"model":"m-$PROMPT_ECHO","input":[]})),
    );
    assert!(codex_answer(&literal).contains("model: m-$PROMPT_ECHO;"));
    let unnamed = replay_native("codex-config-echo", &codex_turn(json!({"input":[]})));
    assert_eq!(
        codex_answer(&unnamed),
        "This turn/start named model: none, so the thread's own; effort: none, so the thread's own."
    );
}

#[test]
fn the_custom_acp_commands_journey_lists_sources_and_echoes_each_command() {
    let prompt = |id: u64, text: &str| json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{"sessionId":"fixture-persisted-session-1","prompt":[{"type":"text","text":text}]}});
    let frames = replay_native(
        "custom-acp-commands",
        &lines(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{}}),
            prompt(3, "/review tests"),
            prompt(4, "/lint"),
            json!({"jsonrpc":"2.0","id":5,"method":"session/close","params":{}}),
        ]),
    );
    let listed = frames
        .iter()
        .map(|frame| &frame["params"]["update"])
        .find(|update| update["sessionUpdate"] == "available_commands_update")
        .expect("commands listed");
    let sources: Vec<_> = listed["availableCommands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["_meta"]["source"].as_str().unwrap_or("none"))
        .collect();
    assert_eq!(sources, ["prompt", "prompt", "skill", "mcp", "none"]);
    let said: Vec<_> = frames
        .iter()
        .filter_map(|frame| frame["params"]["update"]["content"]["text"].as_str())
        .collect();
    assert!(said[0].contains("```text\n/review tests\n```"));
    assert!(said[1].contains("```text\n/lint\n```"));
}

fn pi(id: &str, request: Value) -> Value {
    let mut request = request;
    request["id"] = json!(id);
    request
}

#[test]
fn the_pi_commands_journey_lists_its_commands_and_echoes_each_prompt() {
    let frames = replay_native(
        "pi-commands",
        &lines(&[
            pi("1", json!({"type":"get_state"})),
            pi("2", json!({"type":"get_available_models"})),
            pi("3", json!({"type":"get_state"})),
            pi("4", json!({"type":"get_commands"})),
            pi("5", json!({"type":"prompt","message":"/skill:lint src"})),
            pi("6", json!({"type":"get_available_thinking_levels"})),
            pi("7", json!({"type":"prompt","message":"/fix-tests"})),
        ]),
    );
    let commands = frames
        .iter()
        .find(|frame| frame["command"] == "get_commands")
        .map(|frame| frame["data"]["commands"].as_array().unwrap())
        .expect("commands answered");
    let sources: Vec<_> = commands
        .iter()
        .map(|command| command["source"].as_str().unwrap())
        .collect();
    assert_eq!(sources, ["prompt", "prompt", "skill", "skill"]);
    assert!(commands[1]["name"].as_str().unwrap().len() > 64);
    assert!(
        commands
            .iter()
            .filter_map(|command| command["path"].as_str())
            .all(|path| !path.contains("$ROOT"))
    );
    let replies: Vec<_> = frames
        .iter()
        .filter(|frame| frame["type"] == "message_end")
        .map(|frame| frame["message"]["content"][0]["text"].as_str().unwrap())
        .collect();
    assert_eq!(replies.len(), 2);
    assert!(replies[0].contains("```text\n/skill:lint src\n```"));
    assert!(replies[1].contains("```text\n/fix-tests\n```"));
}

#[test]
fn the_pi_large_run_ends_with_one_oversized_frame_then_takes_a_second_prompt() {
    let frames = replay_native(
        "pi-large-run-end",
        &lines(&[
            pi("1", json!({"type":"get_state"})),
            pi("2", json!({"type":"get_available_models"})),
            pi("3", json!({"type":"get_state"})),
            pi("4", json!({"type":"prompt","message":"Read the example"})),
            pi("5", json!({"type":"get_available_thinking_levels"})),
            pi("6", json!({"type":"prompt","message":"And again"})),
        ]),
    );
    let size = |frame: &Value| serde_json::to_vec(frame).unwrap().len() as u64 + 1;
    let oversized: Vec<_> = frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| size(frame) > LIMITS.wire.frame_bytes)
        .collect();
    let [(end, frame)] = oversized[..] else {
        panic!("exactly one frame outgrows the limit");
    };
    assert_eq!(frame["type"], "agent_end");
    // Every item of the run closed before it, as Sage requires to skip it.
    let kinds: Vec<_> = frames[..end].iter().map(|frame| &frame["type"]).collect();
    assert_eq!(kinds[kinds.len() - 2..], ["message_end", "turn_end"]);
    let started = kinds
        .iter()
        .filter(|kind| **kind == "tool_execution_start")
        .count();
    let ended = kinds
        .iter()
        .filter(|kind| **kind == "tool_execution_end")
        .count();
    assert!(started > 0 && started == ended, "every read finished");
    let after: Vec<_> = frames[end + 1..]
        .iter()
        .map(|frame| &frame["type"])
        .collect();
    assert_eq!(after[0], "agent_settled");
    assert!(
        after.contains(&&json!("response")),
        "the second prompt is answered"
    );
    assert_eq!(after.last().unwrap().as_str(), Some("agent_settled"));
}

fn run_sdk(steps: &[Value], requests: &[Value]) -> Result<Vec<Value>, super::FixtureError> {
    let mut input = std::io::Cursor::new(lines(requests).into_bytes());
    let mut output = Vec::new();
    let mut state = super::WireState::new();
    for step in steps {
        super::execute_with_io(step, &mut state, &mut input, &mut output, &mut Vec::new())?;
    }
    Ok(String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect())
}

#[test]
fn only_a_waiting_follow_up_is_withdrawn() {
    let follow_up = json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"also"}]},"uuid":"f-1"});
    let cancel = |uuid: &str| json!({"type":"control_request","request_id":format!("c-{uuid}"),"request":{"subtype":"cancel_async_message","message_uuid":uuid}});
    let interrupt =
        json!({"type":"control_request","request_id":"i","request":{"subtype":"interrupt"}});
    let wait = |kind: &str| json!({"op":"awaitClient","kind":kind});
    let lifecycle =
        |state: &str| json!({"op":"emitKnownFrame","shape":"claudeSdkLifecycle","state":state});
    let answers = |frames: &[Value]| -> Vec<Value> {
        frames
            .iter()
            .filter(|frame| frame["type"] == "control_response")
            .map(|frame| frame["response"]["response"].clone())
            .collect()
    };

    let waiting = run_sdk(
        &[
            wait("claudeSdkFollowUp"),
            lifecycle("queued"),
            wait("claudeSdkInterrupt"),
        ],
        &[
            follow_up.clone(),
            cancel("other"),
            cancel("f-1"),
            interrupt.clone(),
        ],
    )
    .unwrap();
    assert_eq!(
        answers(&waiting),
        [
            json!({"cancelled":false}),
            json!({"cancelled":true}),
            json!({})
        ]
    );
    let states: Vec<_> = waiting
        .iter()
        .filter_map(|frame| frame["state"].as_str())
        .collect();
    assert_eq!(states, ["queued", "cancelled"]);

    let running = run_sdk(
        &[
            wait("claudeSdkFollowUp"),
            lifecycle("queued"),
            lifecycle("started"),
            wait("claudeSdkInterrupt"),
        ],
        &[follow_up.clone(), cancel("f-1"), interrupt],
    )
    .unwrap();
    assert_eq!(answers(&running), [json!({"cancelled":false}), json!({})]);
    assert!(
        run_sdk(
            &[
                wait("claudeSdkFollowUp"),
                lifecycle("started"),
                lifecycle("queued")
            ],
            &[follow_up],
        )
        .is_err()
    );
}

#[test]
fn only_a_launch_is_named_as_relaunched() {
    let initialize =
        json!({"type":"control_request","request_id":"1","request":{"subtype":"initialize"}});
    let prompt =
        json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}});
    let step =
        |kind: &str, relaunch: Value| json!({"op":"awaitClient","kind":kind,"relaunch":relaunch});
    assert!(
        run_sdk(
            &[step("claudeSdkInitialize", json!(true))],
            std::slice::from_ref(&initialize)
        )
        .is_ok()
    );
    for (refused, request) in [
        (step("claudeSdkPrompt", json!(true)), prompt),
        (step("claudeSdkInitialize", json!(false)), initialize),
    ] {
        assert_eq!(
            run_sdk(std::slice::from_ref(&refused), &[request]).err(),
            Some(super::FixtureError::InvalidArguments),
            "{refused}"
        );
    }
}

#[test]
fn a_follow_up_is_saved_when_a_run_takes_it_up() {
    let root = std::env::temp_dir().join(format!("sage-follow-up-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("project")).unwrap();
    let session = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";
    let mut state = super::WireState::new();
    state.claude_sdk = super::claude_sdk::SdkWireState::opened(
        &["--session-id".to_owned(), session.to_owned()],
        &root.join("config"),
        &root.join("project"),
    )
    .unwrap();
    let user = |text: &str, uuid: &str| json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]},"uuid":uuid});
    let reply = |text: &str| json!({"op":"emitKnownFrame","shape":"claudeSdkNative","frame":{"type":"assistant","parent_tool_use_id":null,"session_id":"fixture-session","message":{"role":"assistant","content":[{"type":"text","text":text}]}}});
    let wait = |kind: &str| json!({"op":"awaitClient","kind":kind});
    let lifecycle =
        |state: &str| json!({"op":"emitKnownFrame","shape":"claudeSdkLifecycle","state":state});
    let steps = [
        wait("claudeSdkPrompt"),
        wait("claudeSdkFollowUp"),
        lifecycle("queued"),
        reply("First answer"),
        lifecycle("started"),
        reply("Follow-up answer"),
        lifecycle("completed"),
    ];
    let mut input = std::io::Cursor::new(
        lines(&[user("Question", "p-1"), user("Also this", "f-1")]).into_bytes(),
    );
    for step in &steps {
        super::execute_with_io(
            step,
            &mut state,
            &mut input,
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap();
    }
    let saved = std::fs::read_dir(root.join("config").join("projects"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join(format!("{session}.jsonl"));
    let order: Vec<String> = std::fs::read_to_string(saved)
        .unwrap()
        .lines()
        .map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            entry["message"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        order,
        ["Question", "First answer", "Also this", "Follow-up answer"]
    );
}
