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
    // Without a bridge marker, Pi's user message echoes stay absorbed.
    for text in ["Inspect read-only", "Extension request"] {
        assert!(matches!(
            runtime.ingest(&user(text), &Mapper).unwrap().events(),
            [AdapterEvent::Absorbed]
        ));
    }
}

fn status(key: &str, body: Value) -> Frame {
    frame(
        json!({"type":"extension_ui_request","id":"status-1","method":"setStatus",
        "statusKey":key,"statusText":body.to_string()}),
    )
}

#[test]
fn an_extension_model_switch_updates_the_model_option_and_sages_own_does_not_repeat() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let page = runtime
        .command(AdapterCommand::ModelPage { cursor: None })
        .unwrap();
    let page = deliver(&mut runtime, page);
    let models = json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"},
        {"provider":"anthropic","id":"claude","name":"Claude"}]});
    runtime.ingest(&response(&page, models), &Mapper).unwrap();
    let model = |provider: &str, id: &str| {
        status(
            "sage.pi.model.v1",
            json!({"version":1,"provider":provider,"id":id}),
        )
    };
    let config = |runtime: &PiRuntime| serde_json::to_value(runtime.session_config()).unwrap();
    // Pi reports Sage's own switch too; the model in place keeps its level.
    let before = config(&runtime);
    assert_eq!(before["configOptions"][1]["currentValue"], "high");
    let batch = runtime.ingest(&model("openai", "gpt"), &Mapper).unwrap();
    assert!(batch.events().is_empty() && batch.egress().is_empty());
    assert_eq!(config(&runtime), before);
    runtime
        .ingest(&model("anthropic", "claude"), &Mapper)
        .unwrap();
    let after = config(&runtime);
    assert_eq!(
        after["configOptions"][0]["currentValue"],
        "anthropic:claude"
    );
    // A model outside the catalogue is read with the next prompt.
    runtime
        .ingest(&model("router", "virtual"), &Mapper)
        .unwrap();
    assert_eq!(config(&runtime), after);
    assert!(runtime.configuration_dirty);
}

#[test]
fn an_extension_sent_user_message_shows_once_per_marker_and_sages_echo_stays_absorbed() {
    use sha2::{Digest, Sha256};
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let marker = |runtime: &mut PiRuntime, text: &str| {
        let digest: String = Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let body = json!({"version":1,"digest":digest});
        let batch = runtime
            .ingest(&status("sage.pi.user.v1", body), &Mapper)
            .unwrap();
        assert!(batch.events().is_empty());
    };
    let shown = |runtime: &mut PiRuntime, text: &str| {
        let batch = runtime.ingest(&user(text), &Mapper).unwrap();
        match batch.events() {
            [
                AdapterEvent::Item {
                    payload: NormalizedPayload::UserPrompt { text: shown },
                    ..
                },
            ] => {
                assert_eq!(shown.as_str(), text);
                true
            }
            [AdapterEvent::Absorbed] => false,
            events => panic!("{events:?}"),
        }
    };
    assert!(!shown(&mut runtime, "Inspect read-only"));
    // One text sent twice is two markers: two turns, then Sage's own echo.
    marker(&mut runtime, "Again");
    marker(&mut runtime, "Again");
    assert!(shown(&mut runtime, "Again"));
    assert!(shown(&mut runtime, "Again"));
    assert!(!shown(&mut runtime, "Again"));
    // Past the per-turn cap the oldest marker is dropped.
    for index in 0..9 {
        marker(&mut runtime, &format!("queued {index}"));
    }
    assert!(!shown(&mut runtime, "queued 0"));
    assert!(shown(&mut runtime, "queued 8"));
    assert!(shown(&mut runtime, "queued 1"));
    // A marker whose message never came (another input handler swallowed
    // it) does not claim Sage's own steer of the same text.
    marker(&mut runtime, "Use the cache");
    let steer = runtime
        .command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", "Use the cache").unwrap(),
            client_message_id: RequiredText::new("client", "steer").unwrap(),
        })
        .unwrap();
    deliver(&mut runtime, steer);
    assert!(!shown(&mut runtime, "Use the cache"));
    // Pi ends the run only once both queues drain, so what is left is stale.
    runtime
        .ingest(&frame(json!({"type":"agent_end","messages":[]})), &Mapper)
        .unwrap();
    assert!(!shown(&mut runtime, "queued 2"));
}

#[test]
fn extension_messages_past_the_turn_cap_add_one_notice_and_then_nothing() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    assert!(matches!(
        runtime
            .ingest(
                &end(json!({"role":"custom","content":"no type","display":true})),
                &Mapper
            )
            .unwrap()
            .events(),
        [AdapterEvent::Unsupported(_)]
    ));
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
