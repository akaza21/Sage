use super::*;
use sage_acp_protocol::{intent::user_input::UserInputAnswers, permissions::PermissionDecision};

fn dialog(id: &str) -> Value {
    json!({"type":"extension_ui_request","id":id,"method":"input","title":"Name?"})
}

#[test]
fn idle_questions_adopt_without_model_authority_and_the_fifo_is_bounded() {
    let mut runtime = fixture_runtime(87);
    ready(&mut runtime);
    let start = runtime.ingest(&frame(dialog("visible")), &Mapper).unwrap();
    assert!(matches!(start.events(), [AdapterEvent::TurnStarted { .. }]));
    let first = runtime
        .adopt_turn(
            RequiredText::new("client", "idle-question").unwrap(),
            &Mapper,
        )
        .unwrap();
    assert!(
        matches!(first.events(), [AdapterEvent::PermissionRequest { external_request_id, .. }] if external_request_id.as_str() == "visible")
    );
    // Without a model turn, unsolicited execution cannot acquire tool authority.
    let tool = runtime.ingest(&frame(json!({"type":"tool_execution_start","toolCallId":"unsolicited","toolName":"write","args":{}})), &Mapper).unwrap();
    assert!(
        !tool
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::Item { .. }))
    );
    for id in ["one", "two", "three", "four"] {
        let batch = runtime.ingest(&frame(dialog(id)), &Mapper).unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
        assert!(batch.egress().is_empty());
    }
    let overflow = runtime.ingest(&frame(dialog("overflow")), &Mapper).unwrap();
    assert!(matches!(overflow.events(), [AdapterEvent::Session { .. }]));
    let reply = overflow.into_parts().1.remove(0);
    runtime.commit_mandatory_egress(reply.token()).unwrap();
    assert_eq!(
        deliver(&mut runtime, reply),
        json!({"type":"extension_ui_response","id":"overflow","cancelled":true})
    );
    for (id, next) in [
        ("visible", Some("one")),
        ("one", Some("two")),
        ("two", Some("three")),
        ("three", Some("four")),
        ("four", None),
    ] {
        let reply = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: SageServerRequestId::parse(id).unwrap(),
                decision: PermissionDecision::Answer(
                    serde_json::from_value::<UserInputAnswers>(
                        json!([{"questionId":"answer","text":"Ada"}]),
                    )
                    .unwrap(),
                ),
            })
            .unwrap();
        assert_eq!(
            deliver(&mut runtime, reply),
            json!({"type":"extension_ui_response","id":id,"value":"Ada"})
        );
        let batch = runtime.answers_delivered(&Mapper).unwrap().unwrap();
        if let Some(next) = next {
            assert!(
                matches!(batch.events(), [AdapterEvent::PermissionRequest { external_request_id, .. }] if external_request_id.as_str() == next)
            );
        } else {
            assert!(matches!(
                batch.events(),
                [AdapterEvent::Turn {
                    state: sage_acp_protocol::lifecycle::TurnState::Completed,
                    ..
                }]
            ));
        }
    }
    running(&mut runtime);
}

#[test]
fn over_cap_select_frames_cancel_with_one_notice_and_leave_the_session_usable() {
    let mut runtime = fixture_runtime(88);
    ready(&mut runtime);
    running(&mut runtime);
    for dialog in [
        serde_json::from_slice::<Value>(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/acp/pi/select-over-cap-150.json"
        )))
        .unwrap(),
        json!({"type":"extension_ui_request","id":"too-long","method":"select","title":"Pick","options":["\u{e9}".repeat(129)]}),
    ] {
        let id = dialog["id"].as_str().unwrap().to_owned();
        let incoming = frame(dialog);
        let batch = runtime.ingest(&incoming, &Mapper).unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::Session { .. }]));
        let reply = batch.into_parts().1.remove(0);
        runtime.commit_mandatory_egress(reply.token()).unwrap();
        assert_eq!(
            deliver(&mut runtime, reply),
            json!({"type":"extension_ui_response","id":id,"cancelled":true})
        );
        let replay = runtime.ingest(&incoming, &Mapper).unwrap();
        assert!(matches!(replay.events(), [AdapterEvent::IgnoredDuplicate]));
        assert!(replay.egress().is_empty());
    }
    let value = "é".repeat(128);
    let batch = runtime.ingest(&frame(json!({"type":"extension_ui_request","id":"valid","method":"select","title":"Pick","options":[value]})), &Mapper).unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("valid").unwrap(),
            decision: PermissionDecision::Answer(
                serde_json::from_value(json!([{"questionId":"answer","selectedOption":value}]))
                    .unwrap(),
            ),
        })
        .unwrap();
    assert_eq!(deliver(&mut runtime, reply)["value"], value);
}

#[test]
fn cancelling_an_idle_question_cancels_parallel_arrivals_before_the_reply_is_written() {
    let mut runtime = fixture_runtime(89);
    ready(&mut runtime);
    runtime.ingest(&frame(dialog("stopping")), &Mapper).unwrap();
    runtime
        .adopt_turn(RequiredText::new("client", "idle-stop").unwrap(), &Mapper)
        .unwrap();
    let reply = runtime
        .cancel_permission(&SageServerRequestId::parse("stopping").unwrap())
        .unwrap();
    runtime.commit_mandatory_egress(reply.token()).unwrap();
    let arrived = runtime.ingest(&frame(dialog("parallel")), &Mapper).unwrap();
    assert!(
        !arrived
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
    );
    let parallel = arrived.into_parts().1.remove(0);
    runtime.commit_mandatory_egress(parallel.token()).unwrap();
    assert_eq!(deliver(&mut runtime, parallel)["cancelled"], true);
    assert!(runtime.answers_delivered(&Mapper).unwrap().is_none());
    assert_eq!(deliver(&mut runtime, reply)["cancelled"], true);
    let ended = runtime.answers_delivered(&Mapper).unwrap().unwrap();
    assert!(matches!(
        ended.events(),
        [
            AdapterEvent::Session { .. },
            AdapterEvent::Turn {
                state: sage_acp_protocol::lifecycle::TurnState::Cancelled,
                ..
            }
        ]
    ));
    assert!(format!("{:?}", ended.events()).contains("Pi: 1 waiting question cancelled."));
}

#[test]
fn stopping_an_idle_question_turn_reports_its_waiting_questions_in_one_notice() {
    let mut runtime = fixture_runtime(94);
    ready(&mut runtime);
    runtime.ingest(&frame(dialog("visible")), &Mapper).unwrap();
    runtime
        .adopt_turn(RequiredText::new("client", "idle-stop-3").unwrap(), &Mapper)
        .unwrap();
    for id in ["one", "two", "three"] {
        runtime.ingest(&frame(dialog(id)), &Mapper).unwrap();
    }
    let stop = runtime.command(AdapterCommand::Cancel).unwrap();
    runtime.commit_mandatory_egress(stop.token()).unwrap();
    assert_eq!(
        deliver(&mut runtime, stop),
        json!({"type":"extension_ui_response","id":"visible","cancelled":true})
    );
    let ended = runtime.answers_delivered(&Mapper).unwrap().unwrap();
    let transcript = format!("{:?}", ended.events());
    assert!(matches!(
        ended.events(),
        [
            AdapterEvent::Session { .. },
            AdapterEvent::Turn {
                state: sage_acp_protocol::lifecycle::TurnState::Cancelled,
                ..
            }
        ]
    ));
    assert!(transcript.contains("Pi: 3 waiting questions cancelled."));
    assert!(!transcript.contains("not supported"));
    let (_, replies) = ended.into_parts();
    let mut cancelled = Vec::new();
    for reply in replies {
        runtime.commit_mandatory_egress(reply.token()).unwrap();
        cancelled.push(deliver(&mut runtime, reply));
    }
    assert_eq!(
        cancelled,
        ["one", "two", "three"]
            .map(|id| json!({"type":"extension_ui_response","id":id,"cancelled":true}))
    );
}

#[test]
fn refusal_notices_are_budgeted_per_turn_not_per_runtime() {
    let mut runtime = fixture_runtime(95);
    ready(&mut runtime);
    let over_cap = |id: &str| {
        frame(
            json!({"type":"extension_ui_request","id":id,"method":"select","title":"Pick","options":["\u{e9}".repeat(129)]}),
        )
    };
    let refuse = |runtime: &mut PiRuntime, id: &str| {
        let batch = runtime.ingest(&over_cap(id), &Mapper).unwrap();
        let noticed = matches!(batch.events(), [AdapterEvent::Session { .. }]);
        let reply = batch.into_parts().1.remove(0);
        runtime.commit_mandatory_egress(reply.token()).unwrap();
        assert_eq!(deliver(runtime, reply)["cancelled"], true);
        noticed
    };
    running(&mut runtime);
    for at in 0..8 {
        assert!(refuse(&mut runtime, &format!("first-{at}")));
    }
    assert!(!refuse(&mut runtime, "first-over"));
    for event in ["agent_end", "agent_settled"] {
        runtime
            .ingest(&frame(json!({"type":event,"messages":[]})), &Mapper)
            .unwrap();
    }
    running(&mut runtime);
    assert!(refuse(&mut runtime, "second-turn"));
}

#[test]
fn deadlines_expire_queued_questions_without_waiting_for_the_visible_answer() {
    let mut runtime = fixture_runtime(90);
    let clock = QuestionClock::new(&mut runtime);
    ready(&mut runtime);
    running(&mut runtime);
    runtime.ingest(&frame(dialog("visible")), &Mapper).unwrap();
    for at in 0..4 {
        let mut timed = dialog(&format!("timed-{at}"));
        timed["timeout"] = json!(20);
        runtime.ingest(&frame(timed), &Mapper).unwrap();
    }
    assert_eq!(
        runtime.next_request_deadline(),
        Some(clock.now() + std::time::Duration::from_millis(20))
    );
    clock.advance_ms(19);
    assert!(runtime.poll_request_deadlines(&Mapper).unwrap().is_none());
    clock.advance_ms(1);
    let expired = runtime.poll_request_deadlines(&Mapper).unwrap().unwrap();
    assert_eq!(expired.egress().len(), 4);
    for reply in expired.into_parts().1 {
        runtime.commit_mandatory_egress(reply.token()).unwrap();
        assert_eq!(deliver(&mut runtime, reply)["cancelled"], true);
    }
    assert!(runtime.next_request_deadline().is_none());
    let admitted = runtime.ingest(&frame(dialog("next")), &Mapper).unwrap();
    assert!(admitted.egress().is_empty());
    assert!(matches!(admitted.events(), [AdapterEvent::Absorbed]));
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("visible").unwrap(),
            decision: PermissionDecision::Answer(
                serde_json::from_value(json!([{"questionId":"answer","text":"Ada"}])).unwrap(),
            ),
        })
        .unwrap();
    deliver(&mut runtime, reply);
    assert!(
        matches!(runtime.answers_delivered(&Mapper).unwrap().unwrap().events(), [AdapterEvent::PermissionRequest { external_request_id, .. }] if external_request_id.as_str() == "next")
    );
}

#[test]
fn an_expired_queued_question_is_cancelled_before_the_next_card_opens() {
    let mut runtime = fixture_runtime(93);
    let clock = QuestionClock::new(&mut runtime);
    ready(&mut runtime);
    running(&mut runtime);
    runtime.ingest(&frame(dialog("visible")), &Mapper).unwrap();
    let mut timed = dialog("expired");
    timed["timeout"] = json!(20);
    runtime.ingest(&frame(timed), &Mapper).unwrap();
    runtime.ingest(&frame(dialog("next")), &Mapper).unwrap();
    clock.advance_ms(20);
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("visible").unwrap(),
            decision: PermissionDecision::Answer(
                serde_json::from_value(json!([{"questionId":"answer","text":"Ada"}])).unwrap(),
            ),
        })
        .unwrap();
    deliver(&mut runtime, reply);
    let next = runtime.answers_delivered(&Mapper).unwrap().unwrap();
    assert!(
        matches!(next.events(), [AdapterEvent::PermissionRequest { external_request_id, .. }] if external_request_id.as_str() == "next")
    );
    assert_eq!(next.egress().len(), 1);
    let expired = next.into_parts().1.remove(0);
    runtime.commit_mandatory_egress(expired.token()).unwrap();
    assert_eq!(
        deliver(&mut runtime, expired),
        json!({"type":"extension_ui_response","id":"expired","cancelled":true})
    );
}

#[test]
fn a_dialog_that_expired_before_idle_adoption_never_opens_a_card() {
    let mut runtime = fixture_runtime(92);
    let clock = QuestionClock::new(&mut runtime);
    ready(&mut runtime);
    let mut timed = dialog("expired");
    timed["timeout"] = json!(20);
    runtime.ingest(&frame(timed), &Mapper).unwrap();
    clock.advance_ms(20);
    let adopted = runtime
        .adopt_turn(
            RequiredText::new("client", "expired-idle").unwrap(),
            &Mapper,
        )
        .unwrap();
    assert!(adopted.events().is_empty());
    let reply = adopted.into_parts().1.remove(0);
    runtime.commit_mandatory_egress(reply.token()).unwrap();
    assert_eq!(
        deliver(&mut runtime, reply),
        json!({"type":"extension_ui_response","id":"expired","cancelled":true})
    );
    assert!(matches!(
        runtime
            .answers_delivered(&Mapper)
            .unwrap()
            .unwrap()
            .events(),
        [AdapterEvent::Turn {
            state: sage_acp_protocol::lifecycle::TurnState::Cancelled,
            ..
        }]
    ));
}
