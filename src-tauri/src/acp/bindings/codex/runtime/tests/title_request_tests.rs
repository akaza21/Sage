use super::*;

fn titled_runtime() -> CodexRuntime {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("fixture-model"), Some("low"));
    // The production thread/start reply carries these authority settings.
    runtime.title_settings =
        Some(json!({"model":"fixture-model","modelProvider":"openai","cwd":"."}));
    runtime
}

fn request(runtime: &mut CodexRuntime) -> Value {
    let egress = runtime.poll_title().expect("next title request");
    let value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    value
}

fn absorb(runtime: &mut CodexRuntime, event: Value) {
    let batch = runtime.ingest(&frame(event.clone()), &Mapper).unwrap();
    assert!(
        matches!(batch.events(), [AdapterEvent::Absorbed]),
        "temporary event leaked: {event}"
    );
    assert!(batch.egress().is_empty());
}

#[cfg(feature = "acp-fixture")]
#[test]
fn codex_title_wire_never_enters_the_main_transcript_or_creates_child_chats() {
    let mut runtime = titled_runtime();
    let main = runtime.inner.clone();
    runtime
        .start_title("User:\nExplain empty chats\nAssistant:\nThe sidebar filters need checking")
        .unwrap();
    for method in [
        "config/read",
        "thread/start",
        "turn/start",
        "thread/unsubscribe",
    ] {
        let wire = request(&mut runtime);
        assert_eq!(wire["method"], method);
        if method == "thread/start" {
            assert_eq!(wire["params"]["ephemeral"], true);
            assert_eq!(wire["params"]["model"], "fixture-model");
        }
        for event in crate::acp::fixture::title_wire(&wire) {
            absorb(&mut runtime, event);
        }
    }
    assert_eq!(
        runtime.take_title_result(),
        Some(Some("Investigate empty sidebar chats".into()))
    );
    assert!(!runtime.title_pending());
    assert!(
        runtime.inner == main,
        "the bound conversation and child ledger are unchanged"
    );
    absorb(
        &mut runtime,
        json!({"method":"item/completed","params":{"threadId":"fixture-ephemeral-title", "turnId":"fixture-ephemeral-title-turn","item":{"id":"late","type":"agentMessage","text":"late"}}}),
    );
    assert!(runtime.inner == main);
}

#[test]
fn cancelling_during_start_waits_for_identity_then_detaches_without_sending_a_turn() {
    let mut runtime = titled_runtime();
    runtime
        .start_title("User and final assistant text")
        .unwrap();
    let config = request(&mut runtime);
    absorb(
        &mut runtime,
        json!({"id":config["id"],"result":{"config":{}}}),
    );
    let start = request(&mut runtime);
    runtime.cancel_title();
    runtime.title.as_mut().unwrap().cleanup_deadline = Some(std::time::Instant::now());
    assert!(runtime.poll_title().is_none());
    assert!(
        runtime.title_pending(),
        "an elapsed deadline is not evidence of unload"
    );
    assert!(runtime.title_requires_teardown());
    absorb(
        &mut runtime,
        json!({"id":start["id"],"result":{"thread":{"id":"temporary"},"sandbox":{"type":"readOnly"}}}),
    );
    assert_eq!(request(&mut runtime)["method"], "thread/unsubscribe");
    absorb(
        &mut runtime,
        json!({"method":"thread/closed","params":{"threadId":"temporary"}}),
    );
    assert!(!runtime.title_pending());
    assert_eq!(runtime.take_title_result(), None);
}

#[test]
fn failed_configuration_and_oversized_response_keep_the_main_session_usable() {
    let mut runtime = titled_runtime();
    let main = runtime.inner.clone();
    runtime.start_title("bounded input").unwrap();
    let config = request(&mut runtime);
    absorb(
        &mut runtime,
        json!({"id":config["id"],"error":{"code":-32601,"message":"Unavailable"}}),
    );
    assert!(runtime.poll_title().is_none());
    assert!(!runtime.title_pending());
    assert_eq!(runtime.take_title_result(), Some(None));
    assert!(runtime.inner == main);
    assert!(runtime.start_title(&"x".repeat(4097)).is_err());
}

#[test]
fn definitely_unsent_title_requests_settle_without_tearing_down_the_main_worker() {
    for resolution in [
        EgressResolution::QueueRejected,
        EgressResolution::WriteZeroBytes,
    ] {
        for fail_start in [false, true] {
            let mut runtime = titled_runtime();
            runtime.start_title("bounded input").unwrap();
            if fail_start {
                let config = request(&mut runtime);
                absorb(
                    &mut runtime,
                    json!({"id":config["id"],"result":{"config":{}}}),
                );
            }
            let unsent = runtime.poll_title().unwrap();
            runtime
                .resolve_egress(unsent.into_token(), resolution)
                .unwrap();
            runtime.title.as_mut().unwrap().cleanup_deadline = Some(std::time::Instant::now());
            assert!(runtime.poll_title().is_none());
            assert!(!runtime.title_pending());
            assert!(!runtime.title_requires_teardown());
            assert_eq!(runtime.take_title_result(), Some(None));
            assert!(
                runtime
                    .command(AdapterCommand::Prompt {
                        prompt: Prompt::new("prompt", "Continue the main conversation").unwrap(),
                        client_message_id: RequiredText::new("client", "foreground").unwrap(),
                        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
                    })
                    .is_ok()
            );
        }
    }
}

#[test]
fn uncertain_title_delivery_immediately_blocks_the_main_connection() {
    let mut runtime = titled_runtime();
    runtime.start_title("bounded input").unwrap();
    let partial = runtime.poll_title().unwrap();
    runtime
        .resolve_egress(partial.into_token(), EgressResolution::DeliveryUnknown)
        .unwrap();
    assert!(runtime.title_requires_teardown());
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Continue").unwrap(),
                client_message_id: RequiredText::new("client", "foreground").unwrap(),
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_err()
    );
}

#[test]
fn cancelling_a_sent_config_read_needs_no_remote_cleanup_or_reply() {
    let mut runtime = titled_runtime();
    runtime.start_title("bounded input").unwrap();
    let config = request(&mut runtime);
    runtime.cancel_title();
    runtime.title.as_mut().unwrap().cleanup_deadline = Some(std::time::Instant::now());
    assert!(!runtime.title_pending());
    assert!(!runtime.title_requires_teardown());
    absorb(
        &mut runtime,
        json!({"id":config["id"],"result":{"config":{}}}),
    );
    assert!(
        runtime.poll_title().is_none(),
        "a late read must not create a thread"
    );
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Continue").unwrap(),
                client_message_id: RequiredText::new("client", "foreground").unwrap(),
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_ok()
    );
}

#[test]
fn title_effort_uses_the_actual_models_capabilities_and_rejection_stays_local() {
    for (model, efforts, expected) in [
        ("fixture-model", vec!["medium", "high"], Some("medium")),
        ("fixture-model", vec![], None),
        ("gpt-5.6-luna", vec!["high"], Some("high")),
        ("gpt-5.6-luna", vec!["high", "low"], Some("low")),
    ] {
        let mut runtime = titled_runtime();
        let list = runtime.model_config_hydration().unwrap().unwrap();
        let id = request_id(&list);
        runtime.ingest(&frame(json!({"id":id,"result":{"data":[{
            "id":model,"model":model,"displayName":"Title model","isDefault":true,"hidden":false,
            "supportedReasoningEfforts":efforts.iter().map(|e| json!({"reasoningEffort":e,"description":"Supported"})).collect::<Vec<_>>()
        }],"nextCursor":null}})), &Mapper).unwrap();
        runtime.start_title("bounded input").unwrap();
        let config = request(&mut runtime);
        absorb(
            &mut runtime,
            json!({"id":config["id"],"result":{"config":{}}}),
        );
        let start = request(&mut runtime);
        assert_eq!(start["params"]["model"], model);
        absorb(
            &mut runtime,
            json!({"id":start["id"],"result":{"thread":{"id":"temporary"},"sandbox":{"type":"readOnly"}}}),
        );
        let turn = request(&mut runtime);
        assert_eq!(
            turn["params"].get("effort").and_then(Value::as_str),
            expected
        );
        absorb(
            &mut runtime,
            json!({"id":turn["id"],"error":{"code":-32602,"message":"Unsupported effort"}}),
        );
        assert_eq!(runtime.take_title_result(), Some(None));
        assert_eq!(request(&mut runtime)["method"], "thread/unsubscribe");
        absorb(
            &mut runtime,
            json!({"method":"thread/closed","params":{"threadId":"temporary"}}),
        );
        assert!(!runtime.title_requires_teardown());
        assert!(
            runtime
                .command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "Continue").unwrap(),
                    client_message_id: RequiredText::new("client", "foreground").unwrap(),
                    access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
                })
                .is_ok()
        );
    }
}
