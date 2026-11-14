//! The `codex-startup-notices` native replay journey, frame for frame,
//! through the Codex runtime: the early config warning is held until the
//! thread binds, and the reroute during the turn updates the model.

use super::*;
use crate::acp::intent::session_event::{SessionNotice, SessionNoticeLevel, SessionScopedEvent};

const THREAD: &str = "fixture-persisted-session-1";

fn journey() -> Vec<Value> {
    let requests = [
        json!({"id":1,"method":"initialize"}),
        json!({"id":2,"method":"thread/start"}),
        json!({"id":3,"method":"thread/goal/get"}),
        json!({"id":4,"method":"turn/start"}),
        json!({"id":5,"method":"thread/unsubscribe"}),
    ]
    .iter()
    .map(|request| format!("{request}\n"))
    .collect::<String>();
    crate::acp::fixture::replay_native("codex-startup-notices", &requests)
}

/// The fixture's frame named by `method`, or its response to request `id`.
fn take(frames: &[Value], method: &str, id: u64) -> Value {
    frames
        .iter()
        .find(|frame| match method {
            "" => frame["id"] == id,
            method => frame["method"] == method,
        })
        .unwrap_or_else(|| panic!("the journey sends {method}{id}"))
        .clone()
}

/// Ingests `value` as the answer to `egress`, the request Sage just sent.
fn answer(
    runtime: &mut CodexRuntime,
    egress: AdapterEgress,
    mut value: Value,
) -> Vec<AdapterEvent> {
    value["id"] = request_id(&egress).into();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    ingest(runtime, value)
}

fn ingest(runtime: &mut CodexRuntime, value: Value) -> Vec<AdapterEvent> {
    runtime
        .ingest(&frame(value.clone()), &Mapper)
        .unwrap_or_else(|error| panic!("refused {value}: {error:?}"))
        .events()
        .to_vec()
}

fn session_event(event: &AdapterEvent) -> SessionScopedEvent {
    let AdapterEvent::Session { id, event } = event else {
        panic!("expected a session event, got {event:?}")
    };
    assert_eq!(id.external_id.as_str(), THREAD);
    assert_eq!(id.turn_id, None);
    event.clone()
}

fn warning(source: &str, text: &str) -> SessionScopedEvent {
    SessionScopedEvent::Notice(SessionNotice {
        level: SessionNoticeLevel::Warning,
        text: RequiredText::new("text", text).unwrap(),
        source: Some(RequiredText::new("source", source).unwrap()),
    })
}

#[test]
fn startup_notices_journey_delivers_the_early_config_warning_on_bind_then_the_reroute() {
    let frames = journey();
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    assert!(matches!(
        ingest(&mut runtime, take(&frames, "configWarning", 0)).as_slice(),
        [AdapterEvent::IgnoredLate]
    ));

    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let bound = answer(&mut runtime, start, take(&frames, "", 2));
    let [AdapterEvent::SessionReady { .. }, held] = bound.as_slice() else {
        panic!("the held warning follows the bind: {bound:?}")
    };
    assert_eq!(
        session_event(held),
        warning(
            "codex config",
            "Codex configuration problem: Invalid synthetic configuration; using defaults.\n\
             In config.toml, line 3\n\
             unknown variant `turbo` for key `model_reasoning_effort`"
        )
    );

    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    answer(&mut runtime, prompt, take(&frames, "", 4));
    ingest(&mut runtime, take(&frames, "turn/started", 0));
    let rerouted = ingest(&mut runtime, take(&frames, "model/rerouted", 0));
    assert_eq!(
        rerouted.iter().map(session_event).collect::<Vec<_>>(),
        [
            SessionScopedEvent::TurnModel {
                model_id: RequiredText::new("model", "fixture-model-safe").unwrap(),
            },
            warning(
                "codex model",
                "Codex answered with fixture-model-safe instead of the requested fixture-model \
                 because it flagged possible high-risk cybersecurity activity."
            ),
        ]
    );
    ingest(&mut runtime, take(&frames, "turn/completed", 0));
}
