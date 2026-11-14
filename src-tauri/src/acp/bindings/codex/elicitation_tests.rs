use serde_json::{Value, json};

use crate::{
    acp::{
        intent::{
            NormalizedPayload,
            session_event::SessionScopedEvent,
            user_input::{UserInputAnswers, UserInputKind},
        },
        lifecycle::ServerRequestState,
    },
    acp_protocol::permissions::PermissionDecision,
};

use super::IncomingEvent;
use super::hard_failure_tests::{adapter, ingest, ready, running_turn};
use super::incoming::IngressDisposition;
use super::state::CodexAdapter;

const FORM_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/mcp-elicitation-form.json");
const URL_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/mcp-elicitation-url.json");

fn live() -> CodexAdapter {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    adapter
}

fn fixture(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn asked(adapter: &mut CodexAdapter, frame: &Value) -> NormalizedPayload {
    let IngressDisposition::Events(events) = ingest(adapter, frame) else {
        panic!("an elicitation must wait for the user");
    };
    let [IncomingEvent::Permission { payload, .. }] = events.as_slice() else {
        panic!("expected one pending question request, got {events:?}");
    };
    payload.clone()
}

fn immediate(adapter: &mut CodexAdapter, frame: &Value) -> Value {
    let IngressDisposition::Events(events) = ingest(adapter, frame) else {
        panic!("the request must be answered");
    };
    let [IncomingEvent::HostToolReply { bytes }] = events.as_slice() else {
        panic!("expected exactly one wire reply, got {events:?}");
    };
    serde_json::from_slice(bytes).unwrap()
}

fn decide(adapter: &mut CodexAdapter, id: u64, decision: &PermissionDecision) -> Value {
    let (_, bytes, _) = adapter
        .resolve_pending_permission(&format!("number:{id}"), decision)
        .unwrap()
        .expect("a decided elicitation has a reply");
    serde_json::from_slice(&bytes).unwrap()
}

fn answers(value: Value) -> PermissionDecision {
    PermissionDecision::Answer(serde_json::from_value::<UserInputAnswers>(value).unwrap())
}

#[test]
fn a_form_elicitation_is_asked_on_the_question_card_and_accepted_with_typed_content() {
    let mut adapter = live();
    let NormalizedPayload::PermissionRequest {
        summary,
        reason,
        user_input: Some(request),
        ..
    } = asked(&mut adapter, &fixture(FORM_FIXTURE))
    else {
        panic!("the form must be shown as questions");
    };
    assert_eq!(summary.as_str(), "Answer an MCP server's questions");
    assert_eq!(
        reason.unwrap().as_str(),
        "tickets: Fill in the ticket before it is filed."
    );
    let questions = request.questions.as_slice();
    let ids: Vec<_> = questions.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids, ["title", "priority", "notify", "estimate"]);
    assert_eq!(
        questions[0].limits.max_length,
        Some(80),
        "null rules mean none"
    );
    assert!(!questions[1].optional && questions[2].optional);
    assert_eq!(questions[3].kind, UserInputKind::Integer);

    let reply = decide(
        &mut adapter,
        31,
        &answers(json!([
            {"questionId": "title", "text": "Login fails"},
            {"questionId": "priority", "selectedOption": "Urgent"},
            {"questionId": "notify", "selectedOption": "Yes"},
            {"questionId": "estimate", "text": "8"}
        ])),
    );
    assert_eq!(
        reply,
        json!({"id": 31, "result": {"action": "accept", "content": {
            "title": "Login fails", "priority": "p1", "notify": true, "estimate": 8
        }}})
    );
}

#[test]
fn declining_an_elicitation_replies_decline() {
    let mut adapter = live();
    asked(&mut adapter, &fixture(FORM_FIXTURE));
    assert_eq!(
        decide(&mut adapter, 31, &PermissionDecision::Deny),
        json!({"id": 31, "result": {"action": "decline"}})
    );
}

#[test]
fn a_url_elicitation_is_a_link_request_accepted_without_content() {
    let mut adapter = live();
    let NormalizedPayload::PermissionRequest {
        summary,
        user_input: Some(request),
        ..
    } = asked(&mut adapter, &fixture(URL_FIXTURE))
    else {
        panic!("the link must be shown on the card");
    };
    assert_eq!(summary.as_str(), "Open a link for an MCP server");
    assert!(request.questions.as_slice().is_empty());
    let link = request.url.expect("a link request");
    assert_eq!(
        link.url.as_str(),
        "https://tickets.example.com/authorize?state=abc"
    );
    assert_eq!(link.host.as_str(), "tickets.example.com");
    assert_eq!(
        decide(&mut adapter, 32, &answers(json!([]))),
        json!({"id": 32, "result": {"action": "accept"}})
    );
}

#[test]
fn an_approval_decision_cannot_answer_an_elicitation() {
    let mut adapter = live();
    asked(&mut adapter, &fixture(URL_FIXTURE));
    assert!(
        adapter
            .resolve_pending_permission("number:32", &PermissionDecision::AllowOnce)
            .is_err()
    );
}

#[test]
fn elicitations_sage_cannot_show_are_declined_without_ending_the_session() {
    let mut adapter = live();
    let mut pattern = fixture(FORM_FIXTURE);
    pattern["params"]["requestedSchema"]["properties"]["title"]["pattern"] = json!("^[A-Z]");
    let mut vendor = fixture(FORM_FIXTURE);
    vendor["params"]["mode"] = json!("openai/form");
    let mut verification = fixture(URL_FIXTURE);
    verification["params"] = json!({
        "threadId": "thread-1", "turnId": "turn-1", "serverName": "tickets",
        "mode": "openai/userVerification", "title": "Verify", "description": "Confirm",
        "challenge": "c-1"
    });
    let mut credentials = fixture(URL_FIXTURE);
    credentials["params"]["url"] = json!("https://user:secret@tickets.example.com/");
    for (id, mut frame) in [pattern, vendor, verification, credentials]
        .into_iter()
        .enumerate()
    {
        frame["id"] = json!(40 + id);
        assert_eq!(
            immediate(&mut adapter, &frame),
            json!({"id": 40 + id, "result": {"action": "decline"}})
        );
    }
    running_delta(&mut adapter);
}

#[test]
fn an_elicitation_outside_a_running_turn_is_cancelled() {
    let mut adapter = adapter();
    ready(&mut adapter);
    assert_eq!(
        immediate(&mut adapter, &fixture(FORM_FIXTURE)),
        json!({"id": 31, "result": {"action": "cancel"}})
    );
    let mut adapter = live();
    let mut untied = fixture(URL_FIXTURE);
    untied["params"]["turnId"] = Value::Null;
    assert_eq!(
        immediate(&mut adapter, &untied),
        json!({"id": 32, "result": {"action": "cancel"}})
    );
}

#[test]
fn an_elicitation_before_the_thread_is_bound_is_cancelled_without_ending_the_session() {
    let mut adapter = adapter();
    assert!(adapter.bound_thread_id().is_none());
    assert_eq!(
        immediate(&mut adapter, &fixture(FORM_FIXTURE)),
        json!({"id": 31, "result": {"action": "cancel"}})
    );
    ready(&mut adapter);
    running_turn(&mut adapter);
    running_delta(&mut adapter);
}

#[test]
fn an_elicitation_with_an_unusable_server_name_is_declined_without_ending_the_session() {
    let mut adapter = live();
    for (id, name) in [
        (50, json!("")),
        (51, json!("s".repeat(513))),
        (52, Value::Null),
    ] {
        let mut frame = fixture(FORM_FIXTURE);
        frame["id"] = json!(id);
        frame["params"]["serverName"] = name;
        assert_eq!(
            immediate(&mut adapter, &frame),
            json!({"id": id, "result": {"action": "decline"}})
        );
    }
    running_delta(&mut adapter);
}

#[test]
fn cancelling_a_pending_elicitation_replies_cancel_while_deny_stays_decline() {
    let mut adapter = live();
    asked(&mut adapter, &fixture(FORM_FIXTURE));
    let cancelled: Value =
        serde_json::from_slice(&adapter.cancelled_permission_response("number:31").unwrap())
            .unwrap();
    assert_eq!(cancelled, json!({"id": 31, "result": {"action": "cancel"}}));
    assert_eq!(
        decide(&mut adapter, 31, &PermissionDecision::Deny),
        json!({"id": 31, "result": {"action": "decline"}})
    );
}

#[test]
fn server_request_resolved_withdraws_a_pending_elicitation() {
    let mut adapter = live();
    asked(&mut adapter, &fixture(FORM_FIXTURE));
    let resolved = json!({"method": "serverRequest/resolved", "params": {
        "threadId": "thread-1", "requestId": 31
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &resolved) else {
        panic!("a harness-side resolution must terminalize the card");
    };
    let [IncomingEvent::PermissionResolved { state, .. }] = events.as_slice() else {
        panic!("expected one resolution, got {events:?}");
    };
    assert_eq!(*state, ServerRequestState::Cancelled);
}

#[test]
fn current_time_read_is_answered_with_whole_unix_seconds() {
    let mut adapter = live();
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let frame = json!({"id": 77, "method": "currentTime/read", "params": {"threadId": "thread-1"}});
    let reply = immediate(&mut adapter, &frame);
    assert_eq!(reply["id"], json!(77));
    let now = reply["result"]["currentTimeAt"].as_u64().unwrap();
    assert!((before..before + 5).contains(&now), "{now} is not now");
}

fn startup_failure_notice(adapter: &mut CodexAdapter, failure: Value) -> String {
    let mut frame = json!({"method": "mcpServer/startupStatus/updated", "params": {
        "threadId": "thread-1", "name": "tickets", "status": "failed"
    }});
    for (field, value) in failure.as_object().expect("failure fields") {
        frame["params"][field] = value.clone();
    }
    let IngressDisposition::Events(events) = ingest(adapter, &frame) else {
        panic!("a failed MCP server must be reported");
    };
    let [
        IncomingEvent::Session {
            event: SessionScopedEvent::Notice(notice),
            ..
        },
    ] = events.as_slice()
    else {
        panic!("expected one session notice, got {events:?}");
    };
    running_delta(adapter);
    notice.text.as_str().to_owned()
}

#[test]
fn an_mcp_server_needing_sign_in_becomes_a_notice_not_a_teardown() {
    let mut adapter = live();
    assert_eq!(
        startup_failure_notice(
            &mut adapter,
            json!({"error": "token expired", "failureReason": "reauthenticationRequired"})
        ),
        "The MCP server tickets needs you to sign in again; its tools are unavailable until then."
    );
}

#[test]
fn any_failed_mcp_server_becomes_a_notice_with_its_reason_in_safe_text() {
    let mut adapter = live();
    assert_eq!(
        startup_failure_notice(
            &mut adapter,
            json!({"error": "spawn \u{202e}tickets-mcp\u{0007} failed", "failureReason": null})
        ),
        "The MCP server tickets failed to start: spawn tickets-mcp  failed",
        "bidi controls are dropped and other control characters become spaces"
    );
    assert_eq!(
        startup_failure_notice(&mut adapter, json!({"failureReason": "quotaExceeded"})),
        "The MCP server tickets failed to start."
    );
}

fn running_delta(adapter: &mut CodexAdapter) {
    let delta = json!({"method": "item/agentMessage/delta", "params": {
        "threadId": "thread-1", "turnId": "turn-1", "itemId": "message-1", "delta": "alive"
    }});
    assert!(matches!(
        ingest(adapter, &delta),
        IngressDisposition::Events(_)
    ));
}
