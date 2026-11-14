use super::*;
use crate::{
    acp::{
        intent::{
            PermissionOptionKind,
            user_input::{UserInputAnswers, UserInputKind, UserInputRequest},
        },
        lifecycle::ServerRequestState,
    },
    acp_protocol::permissions::PermissionDecision,
};

fn ask(runtime: &mut PiRuntime, dialog: Value) -> UserInputRequest {
    let batch = runtime.ingest(&frame(dialog), &Mapper).unwrap();
    let [
        AdapterEvent::PermissionRequest {
            payload:
                NormalizedPayload::PermissionRequest {
                    user_input: Some(form),
                    offered_option_kinds: Some(offered),
                    ..
                },
            ..
        },
    ] = batch.events()
    else {
        panic!("question card, got {:?}", batch.events())
    };
    // A question is answered or declined; there is nothing to approve.
    assert_eq!(offered.as_slice(), [PermissionOptionKind::RejectOnce]);
    (**form).clone()
}

fn resolve(runtime: &mut PiRuntime, id: &str, decision: PermissionDecision) -> AdapterEgress {
    runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(id).unwrap(),
            decision,
        })
        .unwrap()
}

fn answers(value: Value) -> PermissionDecision {
    PermissionDecision::Answer(serde_json::from_value::<UserInputAnswers>(value).unwrap())
}

#[test]
fn pi_input_editor_and_select_dialogs_are_answered_on_the_question_card() {
    let mut runtime = fixture_runtime(80);
    ready(&mut runtime);
    running(&mut runtime);

    let form = ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"q-input","method":"input",
            "title":"Branch name?","placeholder":"feature/x"}),
    );
    let question = &form.questions.as_slice()[0];
    assert_eq!(
        (question.question.as_str(), question.kind),
        ("Branch name?", UserInputKind::Text)
    );
    let reply = resolve(
        &mut runtime,
        "q-input",
        answers(json!([{"questionId":"answer","text":"feature/pi"}])),
    );
    assert_eq!(
        deliver(&mut runtime, reply),
        json!({"type":"extension_ui_response","id":"q-input","value":"feature/pi"})
    );

    let form = ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"q-editor","method":"editor",
            "title":"Commit message","prefill":"Fix the parser"}),
    );
    let question = &form.questions.as_slice()[0];
    assert_eq!(
        (
            question.kind,
            question.initial_text.as_ref().map(|text| text.as_str())
        ),
        (UserInputKind::Multiline, Some("Fix the parser"))
    );
    let reply = resolve(
        &mut runtime,
        "q-editor",
        answers(json!([{"questionId":"answer","text":"Fix the parser\n\nAnd its tests."}])),
    );
    assert_eq!(
        deliver(&mut runtime, reply)["value"],
        "Fix the parser\n\nAnd its tests."
    );

    let form = ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"q-select","method":"select",
            "title":"Which target?","options":["debug","release"]}),
    );
    let labels: Vec<_> = form.questions.as_slice()[0]
        .options
        .as_ref()
        .unwrap()
        .as_slice()
        .iter()
        .map(|option| option.label.as_str())
        .collect();
    assert_eq!(labels, ["debug", "release"]);
    let reply = resolve(
        &mut runtime,
        "q-select",
        answers(json!([{"questionId":"answer","selectedOption":"release"}])),
    );
    assert_eq!(deliver(&mut runtime, reply)["value"], "release");
}

#[test]
fn a_declined_question_cancels_only_its_dialog_and_the_turn_goes_on() {
    let mut runtime = fixture_runtime(81);
    ready(&mut runtime);
    running(&mut runtime);
    for id in ["q-1", "q-2"] {
        ask(
            &mut runtime,
            json!({"type":"extension_ui_request","id":id,"method":"input","title":"Name?"}),
        );
        let reply = resolve(&mut runtime, id, PermissionDecision::Deny);
        runtime.commit_mandatory_egress(reply.token()).unwrap();
        assert_eq!(
            deliver(&mut runtime, reply),
            json!({"type":"extension_ui_response","id":id,"cancelled":true})
        );
    }
    // No abort was sent, and the Turn still streams.
    runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Still working."}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { .. },
            ..
        }]
    ));
}

#[test]
fn a_dialog_sage_cannot_show_is_still_cancelled() {
    let mut runtime = fixture_runtime(82);
    ready(&mut runtime);
    running(&mut runtime);
    for dialog in [
        json!({"type":"extension_ui_request","id":"d-1","method":"select","title":"Pick","options":[]}),
        json!({"type":"extension_ui_request","id":"d-2","method":"input"}),
    ] {
        let batch = runtime.ingest(&frame(dialog), &Mapper).unwrap();
        let [egress] = batch.egress() else {
            panic!("cancel reply")
        };
        assert_eq!(request(egress)["cancelled"], true);
    }
}

#[test]
fn a_second_dialog_waits_out_the_open_card_and_the_first_still_answers() {
    let mut runtime = fixture_runtime(83);
    ready(&mut runtime);
    running(&mut runtime);
    ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"first","method":"input","title":"Name?"}),
    );
    // Pi runs tools in parallel; the second dialog must not replace the first.
    let batch = runtime
        .ingest(
            &frame(json!({"type":"extension_ui_request","id":"second","method":"input","title":"Other?"})),
            &Mapper,
        )
        .unwrap();
    let [egress] = batch.egress() else {
        panic!("cancel reply")
    };
    assert_eq!(
        request(egress),
        json!({"type":"extension_ui_response","id":"second","cancelled":true})
    );
    let reply = resolve(
        &mut runtime,
        "first",
        answers(json!([{"questionId":"answer","text":"Ada"}])),
    );
    assert_eq!(deliver(&mut runtime, reply)["value"], "Ada");
}

#[test]
fn a_timed_question_pi_dropped_gives_way_to_the_next_dialog() {
    let mut runtime = fixture_runtime(85);
    ready(&mut runtime);
    running(&mut runtime);
    ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"timed","method":"input","title":"Name?","timeout":1}),
    );
    let batch = runtime
        .ingest(
            &frame(json!({"type":"extension_ui_request","id":"next","method":"input","title":"Other?"})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [
        AdapterEvent::PermissionResolved { external_request_id: expired, state: ServerRequestState::Cancelled, .. },
        AdapterEvent::PermissionRequest { external_request_id, .. },
    ] if expired.as_str() == "timed" && external_request_id.as_str() == "next"));
    assert!(batch.egress().is_empty());
    let reply = resolve(
        &mut runtime,
        "next",
        answers(json!([{"questionId":"answer","text":"Ada"}])),
    );
    assert_eq!(deliver(&mut runtime, reply)["value"], "Ada");
}

#[test]
fn a_card_left_open_when_the_turn_stops_does_not_block_the_next_turn() {
    let mut runtime = fixture_runtime(86);
    ready(&mut runtime);
    running(&mut runtime);
    ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"left-open","method":"input","title":"Name?"}),
    );
    let abort = runtime.command(AdapterCommand::Cancel).unwrap();
    let abort = deliver(&mut runtime, abort);
    runtime
        .ingest(&response(&abort, Value::Null), &Mapper)
        .unwrap();
    for event in ["agent_end", "agent_settled"] {
        runtime
            .ingest(&frame(json!({"type":event,"messages":[]})), &Mapper)
            .unwrap();
    }
    running(&mut runtime);
    ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"next-turn","method":"input","title":"Name?"}),
    );
}

#[test]
fn question_text_is_sanitised_and_unechoable_options_cancel_the_dialog() {
    let mut runtime = fixture_runtime(84);
    ready(&mut runtime);
    running(&mut runtime);
    let form = ask(
        &mut runtime,
        json!({"type":"extension_ui_request","id":"q-editor","method":"editor",
            "title":"Notes","prefill":"Saved at /home/alice/notes.txt"}),
    );
    let prefill = form.questions.as_slice()[0]
        .initial_text
        .as_ref()
        .unwrap()
        .as_str()
        .to_owned();
    assert!(!prefill.contains("/home/alice"), "{prefill}");
    resolve(&mut runtime, "q-editor", PermissionDecision::Deny);
    for (id, options) in [
        ("q-bidi", json!(["safe", "evil\u{202e}txt"])),
        ("q-repeat", json!(["same", "same"])),
    ] {
        let batch = runtime
            .ingest(
                &frame(
                    json!({"type":"extension_ui_request","id":id,"method":"select",
                    "title":"Pick","options":options}),
                ),
                &Mapper,
            )
            .unwrap();
        let [egress] = batch.egress() else {
            panic!("cancel reply for {options}")
        };
        assert_eq!(request(egress)["cancelled"], true);
    }
}
