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
