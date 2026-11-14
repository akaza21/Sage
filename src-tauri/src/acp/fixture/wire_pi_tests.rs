use super::*;
use serde_json::json;
use std::io::Cursor;

#[path = "wire_pi_activation_tests.rs"]
mod activation;

#[test]
fn pi_replay_answers_command_discovery_around_idle_configuration_and_prompts() {
    let mut state = WireState::new();
    let requests = [
        json!({"type":"get_commands","id":"pi-7-1"}),
        json!({"type":"set_thinking_level","id":"pi-7-2","level":"high"}),
        json!({"type":"set_model","id":"pi-7-3","provider":"openai","modelId":"fixture-model-two"}),
        json!({"type":"prompt","id":"pi-7-4","message":"Inspect"}),
        json!({"type":"get_commands","id":"pi-7-5"}),
    ];
    let mut input = Cursor::new(
        requests
            .iter()
            .map(|value| format!("{value}\n"))
            .collect::<String>()
            .into_bytes(),
    );
    let mut output = Vec::new();
    for kind in ["piPrompt", "piIdle"] {
        execute_with_io(
            &json!({"op":"awaitClient","kind":kind}),
            &mut state,
            &mut input,
            &mut output,
            &mut Vec::new(),
        )
        .unwrap();
    }
    let replies: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), requests.len());
    for (request, reply) in requests.iter().zip(&replies) {
        assert_eq!(reply["id"], request["id"]);
        assert_eq!(reply["command"], request["type"]);
        assert_eq!(reply["success"], true);
    }
    assert_eq!(replies[0]["data"], json!({"commands":[]}));
    assert_eq!(replies[2]["data"]["id"], "fixture-model-two");
    assert_eq!(replies[4]["data"], json!({"commands":[]}));
}

#[test]
fn pi_0742_unknown_discovery_command_fixture_omits_the_id() {
    for (kind, command) in [
        (
            "piThinkingLevelsUnsupported",
            "get_available_thinking_levels",
        ),
        ("piCommandsUnsupported", "get_commands"),
    ] {
        let mut state = WireState::new();
        let mut output = Vec::new();
        let request = format!("{}\n", json!({"type":command,"id":"pi-7-6"}));
        execute_with_io(
            &json!({"op":"awaitClient","kind":kind}),
            &mut state,
            &mut Cursor::new(request.into_bytes()),
            &mut output,
            &mut Vec::new(),
        )
        .unwrap();
        let reply: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            reply,
            json!({"type":"response","command":command,"success":false,"error":format!("Unknown command: {command}")})
        );
    }
}

#[test]
fn pi_replay_correlates_string_ids_and_handles_config_before_events() {
    let mut state = WireState::new();
    let mut output = Vec::new();
    let mut input = Cursor::new(b"{\"type\":\"get_state\",\"id\":\"pi-7-1\"}\n{\"type\":\"get_available_models\",\"id\":\"pi-7-2\"}\n{\"type\":\"get_state\",\"id\":\"pi-7-3\"}\n{\"type\":\"prompt\",\"id\":\"pi-7-4\",\"message\":\"Inspect\"}\n{\"type\":\"get_state\",\"id\":\"pi-7-5\"}\n{\"type\":\"get_available_thinking_levels\",\"id\":\"pi-7-6\"}\n");
    for kind in [
        "piInitialize",
        "piModels",
        "piNew",
        "piPrompt",
        "piThinkingLevels",
    ] {
        execute_with_io(
            &json!({"op":"awaitClient","kind":kind}),
            &mut state,
            &mut input,
            &mut output,
            &mut Vec::new(),
        )
        .unwrap();
    }
    let frames: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames.len(), 6);
    assert_eq!(frames[0]["id"], "pi-7-1");
    assert_eq!(frames[1]["command"], "get_available_models");
    assert_eq!(frames[5]["id"], "pi-7-6");
    assert_eq!(
        frames[5]["data"]["levels"],
        json!(["off", "low", "medium", "high"])
    );
}

#[test]
fn pi_replay_rejects_wrong_commands_ids_models_and_dialog_correlation() {
    for (kind, request) in [
        (
            "piInitialize",
            json!({"type":"prompt","id":"pi-1","message":"Inspect"}),
        ),
        ("piInitialize", json!({"type":"get_state","id":7})),
        (
            "piDialogCancelled",
            json!({"type":"extension_ui_response","id":"foreign","cancelled":true}),
        ),
        (
            "piPrompt",
            json!({"type":"set_model","id":"pi-1","provider":"unadmitted","modelId":"other"}),
        ),
    ] {
        let mut state = WireState::new();
        let mut input = Cursor::new(format!("{request}\n").into_bytes());
        assert!(
            execute_with_io(
                &json!({"op":"awaitClient","kind":kind}),
                &mut state,
                &mut input,
                &mut Vec::new(),
                &mut Vec::new()
            )
            .is_err()
        );
    }
}
