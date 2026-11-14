//! Codex `item/tool/requestUserInput` journeys against the harness-neutral
//! question contract: single choice, free-text "other", secret, cancel,
//! interrupt and harness withdrawal.

use serde_json::{Value, json};

use crate::{
    acp::{
        intent::{NormalizedPayload, user_input::UserInputAnswers},
        lifecycle::ServerRequestState,
    },
    acp_protocol::permissions::PermissionDecision,
};

use super::IncomingEvent;
use super::hard_failure_tests::{adapter, ingest, ready, running_turn};
use super::incoming::IngressDisposition;
use super::state::CodexAdapter;

fn ask(adapter: &mut CodexAdapter, id: u64, auto_resolution_ms: Option<u64>) -> NormalizedPayload {
    ask_timed(
        adapter,
        id,
        json!({"isBlocking": auto_resolution_ms.is_none(), "autoResolutionMs": auto_resolution_ms}),
    )
}

/// Asks the three standard questions with the given `isBlocking` and
/// `autoResolutionMs` fields.
fn ask_timed(adapter: &mut CodexAdapter, id: u64, timing: Value) -> NormalizedPayload {
    let mut frame = json!({"id": id, "method": "item/tool/requestUserInput", "params": {
        "threadId": "thread-1", "turnId": "turn-1", "itemId": format!("call-{id}"),
        "questions": [
            {"id": "scope", "header": "Scope", "question": "Which files?", "isOther": false,
             "isSecret": false, "options": [
                {"label": "All", "description": "Every file"},
                {"label": "Changed", "description": "Only changed files"}]},
            {"id": "branch", "header": "Branch", "question": "Which branch?", "isOther": true,
             "isSecret": false, "options": [{"label": "main", "description": "Default"}]},
            {"id": "token", "header": "Token", "question": "API token?", "isOther": false,
             "isSecret": true, "options": null}
        ]
    }});
    for (field, value) in timing.as_object().expect("timing fields") {
        frame["params"][field] = value.clone();
    }
    let IngressDisposition::Events(events) = ingest(adapter, &frame) else {
        panic!("a question request must wait for the user");
    };
    let [IncomingEvent::Permission { payload, .. }] = events.as_slice() else {
        panic!("expected exactly one pending question request, got {events:?}");
    };
    payload.clone()
}

fn answers(value: Value) -> PermissionDecision {
    PermissionDecision::Answer(serde_json::from_value::<UserInputAnswers>(value).unwrap())
}

fn reply(adapter: &mut CodexAdapter, id: u64, decision: &PermissionDecision) -> Value {
    let (_, bytes, _) = adapter
        .resolve_pending_permission(&format!("number:{id}"), decision)
        .unwrap()
        .expect("a decided question has a reply");
    serde_json::from_slice(&bytes).unwrap()
}

fn live() -> CodexAdapter {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    adapter
}

#[test]
fn a_question_request_projects_bounded_questions_that_only_answers_or_cancel_resolve() {
    let mut adapter = live();
    let NormalizedPayload::PermissionRequest {
        offered_option_kinds,
        user_input: Some(request),
        ..
    } = ask(&mut adapter, 71, Some(30_000))
    else {
        panic!("the request must carry its questions");
    };
    assert_eq!(request.questions.as_slice().len(), 3);
    assert!(request.questions.as_slice()[1].allows_other);
    assert!(request.questions.as_slice()[2].secret);
    assert_eq!(request.auto_resolution_window_ms(), Some(30_000));
    assert_eq!(
        offered_option_kinds.unwrap().as_slice(),
        [crate::acp::intent::PermissionOptionKind::RejectOnce]
    );
    for wrong in [
        PermissionDecision::AllowOnce,
        PermissionDecision::AllowAlways,
    ] {
        assert!(
            adapter
                .resolve_pending_permission("number:71", &wrong)
                .is_err(),
            "a question is answered, never allowed"
        );
    }
}

#[test]
fn a_request_keeps_waiting_unless_codex_names_a_window() {
    for (timing, window) in [
        (json!({"isBlocking": false, "autoResolutionMs": null}), None),
        (json!({"isBlocking": false}), None),
        (json!({"isBlocking": true, "autoResolutionMs": null}), None),
        (json!({}), None),
        (
            json!({"isBlocking": false, "autoResolutionMs": 5000}),
            Some(5000),
        ),
    ] {
        let mut adapter = live();
        let NormalizedPayload::PermissionRequest {
            user_input: Some(request),
            ..
        } = ask_timed(&mut adapter, 79, timing.clone())
        else {
            panic!("the request must carry its questions");
        };
        assert_eq!(request.auto_resolution_window_ms(), window, "{timing}");
    }
}

#[test]
fn a_single_choice_answer_replies_with_the_offered_label() {
    let mut adapter = live();
    ask(&mut adapter, 72, None);
    let reply = reply(
        &mut adapter,
        72,
        &answers(json!([{"questionId": "scope", "selectedOption": "Changed"}])),
    );
    assert_eq!(
        reply,
        json!({"id": 72, "result": {"answers": {"scope": {"answers": ["Changed"]}}}})
    );
    assert!(
        adapter
            .resolve_pending_permission("number:72", &answers(json!([])))
            .is_err(),
        "a question is answered once"
    );
}

#[test]
fn free_text_other_and_secret_answers_reply_as_codex_user_notes() {
    let mut adapter = live();
    ask(&mut adapter, 73, None);
    let reply = reply(
        &mut adapter,
        73,
        &answers(json!([
            {"questionId": "branch", "text": " release/1.2 "},
            {"questionId": "token", "text": "s3cret-token"}
        ])),
    );
    assert_eq!(
        reply["result"]["answers"],
        json!({
            "branch": {"answers": ["user_note: release/1.2"]},
            "token": {"answers": ["user_note: s3cret-token"]}
        })
    );
    // The secret leaves only in the reply frame: the projected request never
    // held it and the retained decline bytes are cleared on answer.
    assert!(adapter.cancelled_permission_response("number:73").is_err());
}

#[test]
fn cancel_and_turn_interrupt_send_the_empty_answer_decline() {
    let mut adapter = live();
    ask(&mut adapter, 74, None);
    let interrupt: Value =
        serde_json::from_slice(&adapter.cancelled_permission_response("number:74").unwrap())
            .unwrap();
    assert_eq!(interrupt, json!({"id": 74, "result": {"answers": {}}}));
    let cancel = reply(&mut adapter, 74, &PermissionDecision::Deny);
    assert_eq!(cancel, interrupt);
}

#[test]
fn server_request_resolved_withdraws_a_question_card() {
    let mut adapter = live();
    ask(&mut adapter, 76, None);
    let resolved = json!({"method": "serverRequest/resolved", "params": {
        "threadId": "thread-1", "requestId": 76
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &resolved) else {
        panic!("a harness-side resolution must withdraw the question");
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::PermissionResolved {
            state: ServerRequestState::Cancelled,
            ..
        }]
    ));
}

#[test]
fn unanswerable_questions_are_declined_at_once() {
    let question = |id: &str| json!({"id": id, "header": "", "question": "?", "options": null});
    let over_cap = (0..9)
        .map(|index| question(&format!("q{index}")))
        .collect::<Vec<_>>();
    let repeated_id = vec![question("q"), question("q")];
    for (id, questions) in [(77_u64, over_cap), (78, repeated_id)] {
        let mut adapter = live();
        let frame = json!({"id": id, "method": "item/tool/requestUserInput", "params": {
            "threadId": "thread-1", "turnId": "turn-1", "itemId": format!("call-{id}"),
            "questions": questions
        }});
        let IngressDisposition::Events(events) = ingest(&mut adapter, &frame) else {
            panic!("request {id} must still be answered");
        };
        let [IncomingEvent::HostToolReply { bytes }] = events.as_slice() else {
            panic!("expected one immediate decline for {id}, got {events:?}");
        };
        assert_eq!(
            serde_json::from_slice::<Value>(bytes).unwrap(),
            json!({"id": id, "result": {"answers": {}}})
        );
    }
}

#[test]
fn a_question_with_an_empty_option_list_takes_free_text() {
    let mut adapter = live();
    let frame = json!({"id": 79, "method": "item/tool/requestUserInput", "params": {
        "threadId": "thread-1", "turnId": "turn-1", "itemId": "call-79",
        "questions": [{"id": "why", "header": "Why", "question": "Why?",
            "isOther": false, "isSecret": false, "options": []}]
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &frame) else {
        panic!("the question must wait for the user");
    };
    let [IncomingEvent::Permission { payload, .. }] = events.as_slice() else {
        panic!("expected one pending question, got {events:?}");
    };
    let NormalizedPayload::PermissionRequest {
        user_input: Some(request),
        ..
    } = payload
    else {
        panic!("the request must carry its question");
    };
    assert!(request.questions.as_slice()[0].options.is_none());
    let answered = reply(
        &mut adapter,
        79,
        &answers(json!([{"questionId": "why", "text": "Because"}])),
    );
    assert_eq!(
        answered["result"]["answers"],
        json!({"why": {"answers": ["user_note: Because"]}})
    );
}
