use super::*;
use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};
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
fn codex_menu_negotiates_confirmed_model_and_effort_with_complete_catalog() {
    let mut state = WireState::new();
    let mut input = Cursor::new(concat!(
        r#"{"id":71,"method":"model/list","params":{"cursor":null}}"#,
        "\n"
    ));
    let mut output = Vec::new();
    for step in [
        json!({"op":"awaitClient","kind":"codexModelList"}),
        json!({"op":"emitKnownFrame","shape":"codexModelListEffortResult"}),
    ] {
        execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new()).unwrap();
    }
    let reply: Value = serde_json::from_slice(&output).unwrap();
    let model = &reply["result"]["data"][0];
    assert_eq!(reply["result"]["data"].as_array().unwrap().len(), 2);
    let alternate = &reply["result"]["data"][1];
    assert_eq!(alternate["id"], "fixture-model-fast");
    assert_eq!(alternate["model"], "fixture-model-fast");
    assert_eq!(alternate["displayName"], "Synthetic replay fast");
    assert_eq!(alternate["isDefault"], false);
    assert_eq!(alternate["hidden"], false);
    assert_eq!(alternate["defaultReasoningEffort"], "low");
    assert_eq!(reply["id"], 71);
    assert_eq!(reply["result"]["nextCursor"], Value::Null);
    assert_eq!(model["defaultReasoningEffort"], "medium");
    let evidence: Value = serde_json::from_str(include_str!(
        "../../../testdata/acp/codex/native-capture-wire.json"
    ))
    .unwrap();
    for field in evidence["modelRequired"].as_array().unwrap() {
        assert!(model.get(field.as_str().unwrap()).is_some());
        assert!(alternate.get(field.as_str().unwrap()).is_some());
    }
    assert_eq!(
        model["supportedReasoningEfforts"],
        json!([
            {"reasoningEffort":"low","description":"Quick synthetic replay"},
            {"reasoningEffort":"medium","description":"Balanced synthetic replay"},
            {"reasoningEffort":"high","description":"Thorough synthetic replay"}
        ])
    );
    assert_eq!(
        alternate["supportedReasoningEfforts"],
        model["supportedReasoningEfforts"]
    );
    assert!(output.len() < 1_024);
    use crate::acp::{
        bindings::codex::{CodexAdapter, IncomingEvent, IngressDisposition, parse_packet_metadata},
        contract::{RuntimeEpoch, SageAgentSessionId},
    };
    for case_id in ["codex-model-menu", "codex-effort-menu"] {
        let frames = capture::replay_native_before(
            case_id,
            concat!(
                r#"{"id":1,"method":"initialize"}"#,
                "\n",
                r#"{"id":72,"method":"thread/start","params":{}}"#,
                "\n",
                r#"{"id":71,"method":"model/list","params":{"cursor":null}}"#,
                "\n"
            ),
            "codexGoalGet",
        );
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1]["id"], 72);
        assert_eq!(frames[1]["result"]["model"], "fixture-model-fast");
        assert_eq!(frames[1]["result"]["reasoningEffort"], "high");
    }
    let packet = parse_packet_metadata(include_bytes!(
        "../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    let mut adapter = CodexAdapter::from_metadata_packet(
        &packet,
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("menu-session").unwrap(),
    );
    let resources = NoCodexResources;
    adapter.initialize(1).unwrap();
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"id":1,"result":{
                "userAgent":user_agent("sage", "unknown"),
                "codexHome":MANAGED_CODEX_HOME,
                "platformFamily":"windows","platformOs":"windows"
            }}))
            .unwrap(),
            &resources,
        )
        .unwrap();
    adapter.models(71, None).unwrap();
    let IngressDisposition::Events(events) = adapter.ingest(&output, &resources).unwrap() else {
        panic!("expected projected models");
    };
    let [
        IncomingEvent::Models {
            models,
            next_cursor,
        },
    ] = events.as_slice()
    else {
        panic!("expected one catalog page");
    };
    assert_eq!(next_cursor, &None);
    assert_eq!(models.len(), 2);
    for (model, id, name, default) in [
        (&models[0], "fixture-model", "Synthetic replay", true),
        (
            &models[1],
            "fixture-model-fast",
            "Synthetic replay fast",
            false,
        ),
    ] {
        assert_eq!(model.id.as_str(), id);
        assert_eq!(model.name.as_str(), name);
        assert_eq!(model.is_default, default);
        assert_eq!(
            model
                .supported_efforts
                .iter()
                .map(|effort| effort.id.as_str())
                .collect::<Vec<_>>(),
            ["low", "medium", "high"]
        );
    }
}

struct NoCodexResources;

impl crate::acp::bindings::codex::ResourceMapper for NoCodexResources {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        Err(crate::acp::intent::UnavailableResourceReason::Foreign)
    }
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

#[test]
fn long_transcript_journey_fills_more_than_a_history_page_and_paces_within_budget() {
    let case = super::super::generated_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == "codex-long-transcript-stream")
        .unwrap();
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    assert!(steps.len() as u64 <= LIMITS.fixture.steps_per_case);
    let paused: u64 = steps
        .iter()
        .filter(|step| step["op"] == "pauseMs")
        .map(|step| step["ms"].as_u64().unwrap())
        .sum();
    assert!(paused <= LIMITS.deadlines_ms.fixture_delay_maximum);

    let transcript = steps
        .iter()
        .find(|step| step["shape"] == "codexTranscript")
        .unwrap();
    let mut state = WireState::new();
    state.current_turn_id = Some("fixture-turn-1".into());
    let mut output = Vec::new();
    execute_with_io(
        transcript,
        &mut state,
        &mut std::io::empty(),
        &mut output,
        &mut Vec::new(),
    )
    .unwrap();
    let frames: Vec<Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert!(frames.len() as u64 > LIMITS.durable.history_page_items);
    assert!(frames.iter().all(|frame| {
        frame["method"] == "item/completed"
            && frame["params"]["turnId"] == "fixture-turn-1"
            && frame["params"]["item"]["type"] == "agentMessage"
    }));
}

#[test]
fn a_transcript_step_outside_its_bounds_is_refused() {
    let mut state = WireState::new();
    state.current_turn_id = Some("fixture-turn-1".into());
    for count in [0, codex::TRANSCRIPT_MESSAGES + 1] {
        assert_eq!(
            codex::emit_transcript(count, &mut state, &mut Vec::new()),
            Err(FixtureError::InvalidArguments)
        );
    }
}

#[test]
fn an_idle_skills_read_is_answered_with_no_skills_and_the_case_goes_on() {
    let mut state = WireState::new();
    let mut input = Cursor::new(concat!(
        r#"{"id":5,"method":"skills/list","params":{}}"#,
        "\n",
        r#"{"id":6,"method":"skills/list","params":{"cwds":["C:/repo"],"forceReload":true}}"#,
        "\n",
        r#"{"id":7,"method":"model/list","params":{"cursor":null}}"#,
        "\n"
    ));
    let mut output = Vec::new();
    execute_with_io(
        &json!({"op":"awaitClient","kind":"codexModelList"}),
        &mut state,
        &mut input,
        &mut output,
        &mut Vec::new(),
    )
    .unwrap();
    let replies: Vec<Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(
        replies,
        [
            json!({"jsonrpc":"2.0","id":5,"result":{"data":[]}}),
            json!({"jsonrpc":"2.0","id":6,"result":{"data":[]}}),
        ]
    );
    assert_eq!(state.last_request_id, Some(7));
}
