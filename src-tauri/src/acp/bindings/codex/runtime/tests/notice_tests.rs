use super::*;
use crate::acp::intent::session_event::{
    SESSION_NOTICE_TEXT_BYTES, SessionNotice, SessionNoticeLevel, SessionScopedEvent,
};

fn notify(runtime: &mut CodexRuntime, method: &str, params: Value) -> Vec<AdapterEvent> {
    runtime
        .ingest(&frame(json!({"method":method,"params":params})), &Mapper)
        .expect("notification")
        .events()
        .to_vec()
}

fn rejected(runtime: &mut CodexRuntime, method: &str, params: Value) -> AdapterRuntimeError {
    match runtime.ingest(&frame(json!({"method":method,"params":params})), &Mapper) {
        Ok(batch) => panic!("expected a rejection, got {:?}", batch.events()),
        Err(error) => error,
    }
}

fn one_notice(events: &[AdapterEvent]) -> SessionNotice {
    match session_events(events).as_slice() {
        [SessionScopedEvent::Notice(notice)] => notice.clone(),
        other => panic!("expected one notice, got {other:?}"),
    }
}

/// The session events of a batch, each checked to carry the bound thread's
/// session identity with no Turn scope.
fn session_events(events: &[AdapterEvent]) -> Vec<SessionScopedEvent> {
    events
        .iter()
        .map(|event| match event {
            AdapterEvent::Session { id, event } => {
                assert_eq!(id.external_id.as_str(), "thread-1");
                assert_eq!(id.turn_id, None);
                event.clone()
            }
            other => panic!("expected a session event, got {other:?}"),
        })
        .collect()
}

fn notice(level: SessionNoticeLevel, source: &str, text: &str) -> SessionScopedEvent {
    SessionScopedEvent::Notice(SessionNotice {
        level,
        text: RequiredText::new("text", text).unwrap(),
        source: Some(RequiredText::new("source", source).unwrap()),
    })
}

fn complete_turn(runtime: &mut CodexRuntime) {
    notify(
        runtime,
        "turn/completed",
        json!({"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}),
    );
}

#[test]
fn warning_with_no_active_turn_becomes_a_session_warning() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let events = notify(
        &mut runtime,
        "warning",
        json!({"threadId":"thread-1","message":"  Model metadata is missing; using defaults.  "}),
    );
    assert_eq!(
        session_events(&events),
        [notice(
            SessionNoticeLevel::Warning,
            "codex",
            "Codex warning: Model metadata is missing; using defaults."
        )]
    );
}

#[test]
fn thread_less_notices_before_binding_are_buffered_then_delivered_on_bind() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    for index in 1..=5 {
        assert!(matches!(
            notify(
                &mut runtime,
                "deprecationNotice",
                json!({"summary":format!("Notice {index}.")}),
            )
            .as_slice(),
            [AdapterEvent::IgnoredLate]
        ));
    }
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({"id":request_id(&start),"result":{"thread":{"id":"thread-1"}}})),
            &Mapper,
        )
        .unwrap();
    let [AdapterEvent::SessionReady { .. }, notices @ ..] = batch.events() else {
        panic!("expected the session to bind first: {:?}", batch.events());
    };
    // At most four are held; the oldest is dropped.
    let expected: Vec<_> = (2..=5)
        .map(|index| {
            notice(
                SessionNoticeLevel::Info,
                "codex deprecation",
                &format!("Codex deprecation notice: Notice {index}."),
            )
        })
        .collect();
    assert_eq!(session_events(notices), expected);
    // Once bound, a thread-less warning goes straight to the session.
    assert_eq!(
        session_events(&notify(
            &mut runtime,
            "warning",
            json!({"threadId":null,"message":"Sandbox is degraded."}),
        )),
        [notice(
            SessionNoticeLevel::Warning,
            "codex",
            "Codex warning: Sandbox is degraded."
        )]
    );
}

#[test]
fn guardian_warning_during_a_turn_is_a_session_notice() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let events = notify(
        &mut runtime,
        "guardianWarning",
        json!({"threadId":"thread-1","message":"Automatic approval review is unavailable."}),
    );
    assert_eq!(
        session_events(&events),
        [notice(
            SessionNoticeLevel::Warning,
            "codex approval reviewer",
            "Codex approval reviewer warning: Automatic approval review is unavailable."
        )]
    );
}

#[test]
fn config_warning_names_the_file_line_and_details() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let events = notify(
        &mut runtime,
        "configWarning",
        json!({
            "summary":"Invalid configuration; using defaults.",
            "details":"unknown variant `fast` for key `model_reasoning_effort`",
            "path":"C:\\codex\\config.toml",
            "range":{"start":{"line":4,"column":1},"end":{"line":4,"column":12}}
        }),
    );
    assert_eq!(
        session_events(&events),
        [notice(
            SessionNoticeLevel::Warning,
            "codex config",
            "Codex configuration problem: Invalid configuration; using defaults.\n\
             In config.toml, line 4\n\
             unknown variant `fast` for key `model_reasoning_effort`"
        )]
    );
}

#[test]
fn deprecation_notice_is_an_info_notice_with_its_guidance() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let events = notify(
        &mut runtime,
        "deprecationNotice",
        json!({"summary":"`experimental_instructions_file` is deprecated.","details":null}),
    );
    assert_eq!(
        session_events(&events),
        [notice(
            SessionNoticeLevel::Info,
            "codex deprecation",
            "Codex deprecation notice: `experimental_instructions_file` is deprecated."
        )]
    );
    let events = notify(
        &mut runtime,
        "deprecationNotice",
        json!({"summary":"Old flag.","details":"Use the new flag."}),
    );
    assert_eq!(
        session_events(&events),
        [notice(
            SessionNoticeLevel::Info,
            "codex deprecation",
            "Codex deprecation notice: Old flag.\nUse the new flag."
        )]
    );
}

#[test]
fn a_long_summary_is_clipped_without_hiding_the_details() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let events = notify(
        &mut runtime,
        "configWarning",
        json!({"summary":"\u{e9}".repeat(4_000),"details":"Fix the model key."}),
    );
    let text = one_notice(&events).text.as_str().to_owned();
    assert!(text.len() <= SESSION_NOTICE_TEXT_BYTES);
    assert!(text.starts_with("Codex configuration problem: \u{e9}"));
    assert!(text.ends_with("\u{2026}\nFix the model key."), "{text}");
}

#[test]
fn harness_text_loses_control_bidi_and_zero_width_characters_but_keeps_lines() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let events = notify(
        &mut runtime,
        "warning",
        json!({"threadId":"thread-1","message":"Run\u{202e}gpj.exe\u{7}\nnext\u{200b} line\u{2066}"}),
    );
    assert_eq!(
        one_notice(&events).text.as_str(),
        "Codex warning: Rungpj.exe \nnext line"
    );
}

#[test]
fn reroute_updates_the_effective_model_and_names_both_models() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let params = json!({
        "threadId":"thread-1","turnId":"turn-1","fromModel":"gpt-6-astra",
        "toModel":"gpt-6-astra-mini","reason":"highRiskCyberActivity"
    });
    let expected = [
        SessionScopedEvent::TurnModel {
            model_id: RequiredText::new("model", "gpt-6-astra-mini").unwrap(),
        },
        notice(
            SessionNoticeLevel::Warning,
            "codex model",
            "Codex answered with gpt-6-astra-mini instead of the requested gpt-6-astra \
             because it flagged possible high-risk cybersecurity activity.",
        ),
    ];
    assert_eq!(
        session_events(&notify(&mut runtime, "model/rerouted", params.clone())),
        expected
    );
    // A reroute that lands just after its Turn finished is still truthful.
    complete_turn(&mut runtime);
    assert_eq!(
        session_events(&notify(&mut runtime, "model/rerouted", params)),
        expected
    );
}

#[test]
fn reroute_for_an_unknown_turn_is_ignored_when_idle_and_rejected_while_a_turn_runs() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let params = json!({
        "threadId":"thread-1","turnId":"turn-replayed","fromModel":"a",
        "toModel":"b","reason":"highRiskCyberActivity"
    });
    assert!(matches!(
        notify(&mut runtime, "model/rerouted", params.clone()).as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    running_turn(&mut runtime);
    assert!(matches!(
        rejected(&mut runtime, "model/rerouted", params),
        AdapterRuntimeError::Operation
    ));
}

#[test]
fn model_verification_explains_slower_responses_and_ignores_an_empty_list() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let events = notify(
        &mut runtime,
        "model/verification",
        json!({"threadId":"thread-1","turnId":"turn-1","verifications":["trustedAccessForCyber"]}),
    );
    let verification = one_notice(&events);
    assert_eq!(verification.level, SessionNoticeLevel::Warning);
    assert!(
        verification
            .text
            .as_str()
            .contains("Trusted Access for Cyber")
    );
    assert!(matches!(
        notify(
            &mut runtime,
            "model/verification",
            json!({"threadId":"thread-1","turnId":"turn-1","verifications":[]}),
        )
        .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn safety_buffering_is_identity_checked_and_not_projected() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let params = json!({
        "threadId":"thread-1","turnId":"turn-1","model":"gpt-6-astra",
        "useCases":["cyber"],"reasons":["user_risk"],
        "showBufferingUi":true,"fasterModel":"gpt-6-astra-mini"
    });
    assert!(matches!(
        notify(
            &mut runtime,
            "model/safetyBuffering/updated",
            params.clone()
        )
        .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    let mut missing_turn = params.clone();
    missing_turn.as_object_mut().unwrap().remove("turnId");
    assert!(matches!(
        rejected(&mut runtime, "model/safetyBuffering/updated", missing_turn),
        AdapterRuntimeError::Frame
    ));
    let mut foreign = params;
    foreign["threadId"] = json!("thread-foreign");
    assert!(matches!(
        rejected(&mut runtime, "model/safetyBuffering/updated", foreign),
        AdapterRuntimeError::Operation
    ));
}

#[test]
fn notices_for_a_foreign_thread_are_rejected() {
    for (method, params) in [
        (
            "warning",
            json!({"threadId":"thread-foreign","message":"Not ours."}),
        ),
        (
            "guardianWarning",
            json!({"threadId":"thread-foreign","message":"Not ours."}),
        ),
        (
            "model/rerouted",
            json!({"threadId":"thread-foreign","turnId":"turn-1","fromModel":"a",
                "toModel":"b","reason":"highRiskCyberActivity"}),
        ),
        (
            "model/verification",
            json!({"threadId":"thread-foreign","turnId":"turn-1",
                "verifications":["trustedAccessForCyber"]}),
        ),
    ] {
        let mut runtime = fixture_runtime();
        ready(&mut runtime);
        running_turn(&mut runtime);
        assert!(
            matches!(
                rejected(&mut runtime, method, params),
                AdapterRuntimeError::Operation
            ),
            "{method}"
        );
    }
}

#[test]
fn upstream_additions_are_tolerated() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    assert_eq!(
        session_events(&notify(
            &mut runtime,
            "warning",
            json!({"threadId":"thread-1","message":"Heads up.","severity":"low"}),
        )),
        [notice(
            SessionNoticeLevel::Warning,
            "codex",
            "Codex warning: Heads up."
        )]
    );
    assert_eq!(
        session_events(&notify(
            &mut runtime,
            "model/rerouted",
            json!({"threadId":"thread-1","turnId":"turn-1","fromModel":"a",
                "toModel":"b","reason":"someFutureReason"}),
        ))[1],
        notice(
            SessionNoticeLevel::Warning,
            "codex model",
            "Codex answered with b instead of the requested a for a safety reason."
        )
    );
    assert!(matches!(
        notify(
            &mut runtime,
            "model/verification",
            json!({"threadId":"thread-1","turnId":"turn-1","verifications":["someFutureCheck"]}),
        )
        .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn missing_or_wrongly_typed_required_fields_are_rejected() {
    for (method, params) in [
        ("warning", json!({"threadId":"thread-1"})),
        ("warning", json!({"threadId":"thread-1","message":"   "})),
        ("warning", json!({"threadId":7,"message":"x"})),
        ("guardianWarning", json!({"message":"x"})),
        ("configWarning", json!({"summary":"x","details":7})),
        ("deprecationNotice", json!({"details":"x"})),
        (
            "model/rerouted",
            json!({"threadId":"thread-1","turnId":"turn-1","fromModel":"a","toModel":"b"}),
        ),
        (
            "model/rerouted",
            json!({"threadId":"thread-1","turnId":"turn-1","fromModel":"a",
                "toModel":"","reason":"highRiskCyberActivity"}),
        ),
        (
            "model/verification",
            json!({"threadId":"thread-1","turnId":"turn-1","verifications":"trustedAccessForCyber"}),
        ),
    ] {
        let mut runtime = fixture_runtime();
        ready(&mut runtime);
        running_turn(&mut runtime);
        assert!(
            matches!(
                rejected(&mut runtime, method, params.clone()),
                AdapterRuntimeError::Frame
            ),
            "{method} {params}"
        );
    }
}

const IGNORED_SETTING: &str = "Codex is ignoring 1 unrecognized configuration setting. \
Check for typos or deprecated settings.";
const IGNORED_SETTING_DETAILS: &str =
    "user [path] `computer_use.windows.always_allowed_app_ids` is ignored.";

#[test]
fn one_config_problem_reported_both_ways_is_shown_once_in_either_order() {
    let config = json!({"summary": IGNORED_SETTING, "details": IGNORED_SETTING_DETAILS});
    let generic = json!({"message": format!("{IGNORED_SETTING}\n  {IGNORED_SETTING_DETAILS}")});

    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let first = one_notice(&notify(&mut runtime, "configWarning", config.clone()));
    assert_eq!(first.source.unwrap().as_str(), "codex config");
    assert!(matches!(
        notify(&mut runtime, "warning", generic.clone()).as_slice(),
        [AdapterEvent::IgnoredLate]
    ));

    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let first = one_notice(&notify(&mut runtime, "warning", generic));
    assert_eq!(first.source.unwrap().as_str(), "codex");
    assert!(matches!(
        notify(&mut runtime, "configWarning", config).as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn different_warnings_are_each_shown() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    one_notice(&notify(
        &mut runtime,
        "configWarning",
        json!({"summary": IGNORED_SETTING, "details": null}),
    ));
    let other = one_notice(&notify(
        &mut runtime,
        "warning",
        json!({"message": "Model metadata is missing; using defaults."}),
    ));
    assert_eq!(
        other.text.as_str(),
        "Codex warning: Model metadata is missing; using defaults."
    );
    let second = one_notice(&notify(
        &mut runtime,
        "configWarning",
        json!({"summary": "Invalid configuration; using defaults.", "details": null}),
    ));
    assert_eq!(second.source.unwrap().as_str(), "codex config");
}

#[test]
fn another_setting_with_the_same_summary_is_still_shown() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    one_notice(&notify(
        &mut runtime,
        "warning",
        json!({"message": format!("{IGNORED_SETTING}\n  {IGNORED_SETTING_DETAILS}")}),
    ));
    let other = one_notice(&notify(
        &mut runtime,
        "configWarning",
        json!({"summary": IGNORED_SETTING, "details": "user [path] `model_verbosity_level` is ignored."}),
    ));
    assert!(
        other
            .text
            .as_str()
            .ends_with("`model_verbosity_level` is ignored.")
    );
}

#[test]
fn a_warning_that_only_begins_like_a_config_summary_is_still_shown() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    one_notice(&notify(
        &mut runtime,
        "configWarning",
        json!({"summary": "Invalid configuration", "details": null}),
    ));
    let other = one_notice(&notify(
        &mut runtime,
        "warning",
        json!({"message": "Invalid configuration in the MCP server list; skipping it."}),
    ));
    assert_eq!(other.source.unwrap().as_str(), "codex");
}
