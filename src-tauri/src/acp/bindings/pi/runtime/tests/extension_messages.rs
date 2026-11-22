use super::*;

#[test]
fn extension_message_roles_are_visible_hidden_or_unsupported_without_losing_the_turn() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let message = |role: &str, display| {
        frame(json!({"type":"message_end","message":{
            "role":role,"customType":"fixture-report","content":"<script>plain text</script>",
            "display":display,"details":{"text":"x".repeat(20000),"token":"fixture-secret"}
        }}))
    };
    let visible = runtime.ingest(&message("custom", true), &Mapper).unwrap();
    assert!(
        matches!(visible.events(), [AdapterEvent::Item { payload: NormalizedPayload::ExtensionMessage { label, text, details: Some(details) }, .. }]
        if label.as_str() == "fixture-report" && text.as_str() == "<script>plain text</script>" && details.as_str().len() <= 16384 && details.as_str().ends_with('…'))
    );
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
    let echo = frame(
        json!({"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"Inspect read-only"}]}}),
    );
    assert!(matches!(
        runtime.ingest(&echo, &Mapper).unwrap().events(),
        [AdapterEvent::Absorbed]
    ));
    let injected = frame(
        json!({"type":"message_end","message":{"role":"user","content":"Extension request"}}),
    );
    assert!(
        matches!(runtime.ingest(&injected, &Mapper).unwrap().events(), [AdapterEvent::Item { payload: NormalizedPayload::UserPrompt { text }, .. }] if text.as_str() == "Extension request")
    );
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
    for end in [
        json!({"type":"agent_end","messages":[]}),
        json!({"type":"agent_settled"}),
    ] {
        runtime.ingest(&frame(end), &Mapper).unwrap();
    }
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Next").unwrap(),
            client_message_id: RequiredText::new("client", "client-pi-2").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt = deliver(&mut runtime, prompt);
    let (_, egress) = runtime
        .ingest(&response(&prompt, Value::Null), &Mapper)
        .unwrap()
        .into_parts();
    let state = deliver(&mut runtime, egress.into_iter().next().unwrap());
    assert_eq!(state["type"], "get_state");
    let model =
        json!({"provider":"extension","id":"virtual","name":"Virtual model","contextWindow":12345});
    let (_, egress) = runtime
        .ingest(
            &response(&state, json!({"model":model,"thinkingLevel":"low"})),
            &Mapper,
        )
        .unwrap()
        .into_parts();
    let models = deliver(&mut runtime, egress.into_iter().next().unwrap());
    assert_eq!(models["type"], "get_available_models");
    let (_, egress) = runtime
        .ingest(&response(&models, json!({"models":[model]})), &Mapper)
        .unwrap()
        .into_parts();
    let levels = deliver(&mut runtime, egress.into_iter().next().unwrap());
    assert_eq!(levels["type"], "get_available_thinking_levels");
    runtime
        .ingest(&response(&levels, json!({"levels":["off","low"]})), &Mapper)
        .unwrap();
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(
        config["configOptions"][0]["currentValue"],
        "extension:virtual"
    );
    assert_eq!(config["configOptions"][1]["currentValue"], "low");
}
