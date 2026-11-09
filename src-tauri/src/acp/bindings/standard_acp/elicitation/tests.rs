use serde_json::json;

use super::*;

/// `CreateElicitationRequest` params as claude-agent-acp 0.76 sends them for
/// a one-question AskUserQuestion (`askUserQuestionsToCreateRequest`).
fn ask_params(session: &str) -> Value {
    json!({
        "mode": "form",
        "sessionId": session,
        "toolCallId": "toolu_1",
        "message": "Which branch should I use?",
        "requestedSchema": {"type": "object", "properties": {
            "question_0": {"type": "string", "title": "Branch",
                "oneOf": [{"const": "main", "title": "main"}, {"const": "dev", "title": "dev"}]},
            "question_0_custom": {"type": "string", "title": "Other",
                "_meta": {"_askUserQuestionCustomAnswer": {"questionId": "question_0", "isCustomAnswer": true}}}
        }}
    })
}

#[test]
fn a_form_in_the_running_turn_is_asked_and_its_answer_typed() {
    let Elicitation::Ask { payload, form } = admit(&ask_params("s1"), "s1", true) else {
        panic!("asked");
    };
    let NormalizedPayload::PermissionRequest {
        reason, user_input, ..
    } = *payload
    else {
        panic!("question payload");
    };
    assert_eq!(reason.unwrap().as_str(), "Which branch should I use?");
    assert_eq!(*user_input.unwrap(), *form);
    let answers = serde_json::from_value(json!([
        {"questionId": "question_0", "selectedOption": "dev"}
    ]))
    .unwrap();
    form.admit(&answers).unwrap();
    assert_eq!(
        accepted(&form, &answers),
        json!({"action": "accept", "content": {"question_0": "dev"}})
    );
}

#[test]
fn a_link_is_asked_and_accepted_without_content() {
    let params = json!({"mode": "url", "sessionId": "s1", "elicitationId": "mcp-oauth-1",
        "message": "Authenticate with MCP server docs", "url": "https://auth.example.com/authorize"});
    let Elicitation::Ask { form, .. } = admit(&params, "s1", true) else {
        panic!("asked");
    };
    let none = serde_json::from_value(json!([])).unwrap();
    form.admit(&none).unwrap();
    assert_eq!(accepted(&form, &none), json!({"action": "accept"}));
}

#[test]
fn requests_nothing_can_ask_are_cancelled_and_unshowable_ones_declined() {
    let cancel = Elicitation::Reply(json!({"action": "cancel"}));
    let decline = Elicitation::Reply(json!({"action": "decline"}));
    assert_eq!(admit(&ask_params("s1"), "s1", false), cancel);
    assert_eq!(admit(&ask_params("other"), "s1", true), cancel);
    // Request-scoped (no session) requests have no Turn to own them.
    assert_eq!(
        admit(
            &json!({"mode": "form", "requestId": 4, "message": "m",
            "requestedSchema": {"type": "object", "properties": {}}}),
            "s1",
            true
        ),
        cancel
    );
    for params in [
        json!({"mode": "url", "sessionId": "s1", "elicitationId": "e", "message": "m",
            "url": "file:///etc/hosts"}),
        json!({"mode": "form", "sessionId": "s1", "message": "m",
            "requestedSchema": {"type": "object", "properties": {"f": {"type": "object"}}}}),
        json!({"mode": "future", "sessionId": "s1", "message": "m"}),
    ] {
        assert_eq!(admit(&params, "s1", true), decline, "{params}");
    }
}
