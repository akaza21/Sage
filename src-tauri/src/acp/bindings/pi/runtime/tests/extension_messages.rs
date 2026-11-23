use super::*;

fn end(message: Value) -> Frame {
    frame(json!({"type":"message_end","message":message}))
}

fn user(text: &str) -> Frame {
    end(json!({"role":"user","content":[{"type":"text","text":text}]}))
}

fn settle(runtime: &mut PiRuntime) {
    for end in [
        json!({"type":"agent_end","messages":[]}),
        json!({"type":"agent_settled"}),
    ] {
        runtime.ingest(&frame(end), &Mapper).unwrap();
    }
}

/// Sends a prompt and returns the state read its acceptance makes.
fn prompt_state_read(runtime: &mut PiRuntime, client: &str) -> Value {
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Next").unwrap(),
            client_message_id: RequiredText::new("client", client).unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt = deliver(runtime, prompt);
    let (_, egress) = runtime
        .ingest(&response(&prompt, Value::Null), &Mapper)
        .unwrap()
        .into_parts();
    let state = deliver(runtime, egress.into_iter().next().unwrap());
    assert_eq!(state["type"], "get_state");
    state
}

/// Answers the state read, then the model and level reads it leads to.
fn answer_configuration(runtime: &mut PiRuntime, state: &Value, model: Value, level: &str) {
    let (_, egress) = runtime
        .ingest(
            &response(state, json!({"model":model,"thinkingLevel":level})),
            &Mapper,
        )
        .unwrap()
        .into_parts();
    let models = deliver(runtime, egress.into_iter().next().unwrap());
    assert_eq!(models["type"], "get_available_models");
    let (_, egress) = runtime
        .ingest(&response(&models, json!({"models":[model]})), &Mapper)
        .unwrap()
        .into_parts();
    let levels = deliver(runtime, egress.into_iter().next().unwrap());
    assert_eq!(levels["type"], "get_available_thinking_levels");
    runtime
        .ingest(
            &response(&levels, json!({"levels":["off","low","high"]})),
            &Mapper,
        )
        .unwrap();
}

#[test]
fn extension_message_roles_are_visible_hidden_or_unsupported_without_losing_the_turn() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    // The secret sits in the first 16 KiB, so redaction, not clipping, hides it.
    let message = |role: &str, display| {
        end(json!({
            "role":role,"customType":"fixture-report","content":"<script>plain text</script>",
            "display":display,
            "details":{"authorization":"Bearer fixture-secret","text":"x".repeat(20000)}
        }))
    };
    let visible = runtime.ingest(&message("custom", true), &Mapper).unwrap();
    let [
        AdapterEvent::Item {
            payload:
                NormalizedPayload::ExtensionMessage {
                    label,
                    text,
                    details: Some(details),
                },
            ..
        },
    ] = visible.events()
    else {
        panic!("{:?}", visible.events())
    };
    assert_eq!(label.as_str(), "fixture-report");
    assert_eq!(text.as_str(), "<script>plain text</script>");
    assert!(details.as_str().len() <= 16384);
    assert!(!details.as_str().contains("fixture-secret"), "{details:?}");
    assert!(matches!(
        runtime
            .ingest(&message("custom", false), &Mapper)
            .unwrap()
            .events(),
        [AdapterEvent::Absorbed]
    ));
    assert!(matches!(
        runtime
            .ingest(&message("future-role", true), &Mapper)
            .unwrap()
            .events(),
        [AdapterEvent::Unsupported(_)]
    ));
    // Pi's user messages, its prompt echo or an injected turn, stay as on main.
    for text in ["Inspect read-only", "Extension request"] {
        assert!(matches!(
            runtime.ingest(&user(text), &Mapper).unwrap().events(),
            [AdapterEvent::Absorbed]
        ));
    }
}

#[test]
fn extension_messages_past_the_turn_cap_add_one_notice_and_then_nothing() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let message =
        || end(json!({"role":"custom","customType":"loop","content":"tick","display":true}));
    for _ in 0..64 {
        assert!(matches!(
            runtime.ingest(&message(), &Mapper).unwrap().events(),
            [AdapterEvent::Item { .. }]
        ));
    }
    assert!(matches!(
        runtime.ingest(&message(), &Mapper).unwrap().events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ExtensionMessage { text, .. },
            ..
        }] if text.as_str() == "Further extension messages in this turn are hidden."
    ));
    assert!(matches!(
        runtime.ingest(&message(), &Mapper).unwrap().events(),
        [AdapterEvent::Absorbed]
    ));
}

#[test]
fn extension_thinking_change_is_read_with_the_next_prompt() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    // Pi reports the change; reading it waits for a prompt, so idle and
    // live notification handling stays as it was.
    let batch = runtime
        .ingest(
            &frame(json!({"type":"thinking_level_changed","level":"low"})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
    settle(&mut runtime);
    let state = prompt_state_read(&mut runtime, "client-pi-2");
    let model =
        json!({"provider":"extension","id":"virtual","name":"Virtual model","contextWindow":12345});
    answer_configuration(&mut runtime, &state, model, "low");
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(
        config["configOptions"][0]["currentValue"],
        "extension:virtual"
    );
    assert_eq!(config["configOptions"][1]["currentValue"], "low");
    assert!(!runtime.configuration_dirty);
}

#[test]
fn a_failed_configuration_read_stays_quiet_and_is_retried() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"type":"thinking_level_changed","level":"low"})),
            &Mapper,
        )
        .unwrap();
    settle(&mut runtime);
    let state = prompt_state_read(&mut runtime, "client-pi-3");
    let failed = frame(
        json!({"type":"response","id":state["id"],"command":"get_state",
        "success":false,"error":"busy"}),
    );
    let batch = runtime.ingest(&failed, &Mapper).unwrap();
    assert_eq!(batch.events(), [AdapterEvent::IgnoredLate]);
    assert!(runtime.configuration_dirty);
}
