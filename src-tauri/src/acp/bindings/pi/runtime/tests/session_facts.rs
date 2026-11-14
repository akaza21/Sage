use super::*;
use crate::acp::intent::session_event::{SessionNoticeLevel, SessionScopedEvent};

fn session_event(runtime: &mut PiRuntime, value: Value) -> Option<SessionScopedEvent> {
    match runtime.ingest(&frame(value), &Mapper).unwrap().events() {
        [AdapterEvent::Session { event, .. }] => Some(event.clone()),
        [AdapterEvent::Absorbed | AdapterEvent::IgnoredLate] => None,
        other => panic!("unexpected events {other:?}"),
    }
}

fn notice(event: Option<SessionScopedEvent>) -> (SessionNoticeLevel, String, String) {
    let Some(SessionScopedEvent::Notice(notice)) = event else {
        panic!("notice expected, got {event:?}")
    };
    (
        notice.level,
        notice.text.as_str().to_owned(),
        notice.source.unwrap().as_str().to_owned(),
    )
}

#[test]
fn extension_notices_and_errors_become_session_notices_with_or_without_a_turn() {
    let mut runtime = fixture_runtime(1);
    assert_eq!(
        session_event(
            &mut runtime,
            json!({"type":"extension_error","error":"before the session"})
        ),
        None
    );
    ready(&mut runtime);
    let notify = |kind: &str, message: &str| json!({"type":"extension_ui_request","id":"n","method":"notify","notifyType":kind,"message":message});
    assert_eq!(
        notice(session_event(
            &mut runtime,
            notify("warning", "Low disk space")
        )),
        (
            SessionNoticeLevel::Warning,
            "Low disk space".into(),
            "Pi".into()
        )
    );
    running(&mut runtime);
    assert_eq!(
        notice(session_event(
            &mut runtime,
            notify(
                "info",
                "token sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789\u{202e}"
            )
        )),
        (SessionNoticeLevel::Info, "token _".into(), "Pi".into())
    );
    assert_eq!(
        notice(session_event(
            &mut runtime,
            json!({"type":"extension_error","extensionPath":"/x/ext.ts","event":"tool_call","error":"boom"})
        )),
        (
            SessionNoticeLevel::Error,
            "boom".into(),
            "Pi extension".into()
        )
    );
    assert_eq!(session_event(&mut runtime, notify("error", "  ")), None);
}

#[test]
fn session_name_changes_become_the_harness_title() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let Some(SessionScopedEvent::Title { title: Some(title) }) = session_event(
        &mut runtime,
        json!({"type":"session_info_changed","name":"Fix the parser"}),
    ) else {
        panic!("title expected")
    };
    assert_eq!(title.as_str(), "Fix the parser");
}

#[test]
fn turn_usage_counts_cached_prompt_tokens_against_the_model_context_window() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({"type":"agent_end","messages":[{"role":"assistant","content":[],
                "stopReason":"stop","usage":{"input":10,"output":4,"cacheRead":90,"cacheWrite":5}}]})),
            &Mapper,
        )
        .unwrap();
    assert!(batch.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Usage {
            payload: NormalizedPayload::Usage {
                input_tokens: 105,
                output_tokens: 4,
                context_window: Some(200_000),
            },
            ..
        }
    )));
}

#[test]
fn a_summary_retry_is_a_warning_and_its_progress_events_stay_quiet() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    assert_eq!(
        notice(session_event(
            &mut runtime,
            json!({"type":"summarization_retry_scheduled","attempt":1,"maxAttempts":3,"delayMs":2000,"errorMessage":"terminated"})
        )),
        (
            SessionNoticeLevel::Warning,
            "Pi is retrying a summary (attempt 1 of 3): terminated".into(),
            "Pi".into()
        )
    );
    for event in [
        json!({"type":"summarization_retry_attempt_start","source":"compaction","reason":"threshold"}),
        json!({"type":"summarization_retry_finished"}),
    ] {
        assert_eq!(session_event(&mut runtime, event), None);
    }
}
