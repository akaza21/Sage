use super::*;
use crate::acp::intent::session_event::SessionScopedEvent;

fn session_info(
    runtime: &mut ClaudeRuntime,
    session: &str,
    update: Value,
) -> Result<AdapterBatch, AdapterRuntimeError> {
    let mut update = update;
    update["sessionUpdate"] = json!("session_info_update");
    runtime.ingest(
        &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId": session, "update": update
        }})),
        &Mapper,
    )
}

fn title(batch: &AdapterBatch) -> Option<&str> {
    match batch.events() {
        [
            SharedEvent::Session {
                id,
                event: SessionScopedEvent::Title { title },
            },
        ] => {
            assert_eq!(id.external_id.as_str(), "claude-session-1");
            assert_eq!(id.turn_id, None);
            Some(title.as_str())
        }
        [SharedEvent::Absorbed] => None,
        other => panic!("unexpected events {other:?}"),
    }
}

#[test]
fn generated_title_is_a_session_event_with_or_without_a_prompt() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let idle = session_info(
        &mut runtime,
        "claude-session-1",
        json!({"title":"Refactor the cache","updatedAt":"2026-11-07T12:00:00.000Z"}),
    )
    .unwrap();
    assert_eq!(title(&idle), Some("Refactor the cache"));

    let (_, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let running = session_info(
        &mut runtime,
        "claude-session-1",
        json!({"title":"Cache refactor"}),
    )
    .unwrap();
    assert_eq!(title(&running), Some("Cache refactor"));
}

#[test]
fn metadata_only_and_blank_updates_are_absorbed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    for update in [
        json!({"_meta":{"goal":{"objective":"ship"}}}),
        json!({"title":null}),
        json!({"title":"\n"}),
    ] {
        let batch = session_info(&mut runtime, "claude-session-1", update).unwrap();
        assert_eq!(title(&batch), None);
    }
}

#[test]
fn foreign_session_or_non_string_title_is_a_frame_error() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    assert!(session_info(&mut runtime, "other-session", json!({"title":"x"})).is_err());
    assert!(session_info(&mut runtime, "claude-session-1", json!({"title":7})).is_err());
}
