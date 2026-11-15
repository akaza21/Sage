use serde_json::json;

use super::*;
use sage_acp_protocol::intent::user_input::{UserInputAnswers, UserInputError};

fn answers(value: Value) -> UserInputAnswers {
    serde_json::from_value(value).unwrap()
}

/// The form claude-agent-acp 0.76 builds for a two-question AskUserQuestion.
fn ask_user_question() -> Value {
    let custom = |question: &str| {
        json!({"type": "string", "title": "Other",
            "description": "Type your own answer instead of choosing an option above (optional).",
            "_meta": {"_askUserQuestionCustomAnswer": {"questionId": question, "isCustomAnswer": true}}})
    };
    json!({"type": "object", "properties": {
        "question_0": {"type": "string", "title": "Scope", "description": "Which files?",
            "oneOf": [{"const": "All", "title": "All", "description": "Every file"},
                      {"const": "Changed", "title": "Changed"}]},
        "question_0_custom": custom("question_0"),
        "question_1": {"type": "array", "title": "Checks", "description": "Run which checks?",
            "items": {"anyOf": [{"const": "lint", "title": "Lint"}, {"const": "test", "title": "Tests"}]}},
        "question_1_custom": custom("question_1")
    }})
}

#[test]
fn ask_user_question_folds_each_custom_field_into_its_question() {
    let request = form_request(&ask_user_question()).unwrap();
    let questions = request.questions.as_slice();
    assert_eq!(questions.len(), 2);
    assert!(questions.iter().all(|q| q.optional && q.allows_other));
    assert_eq!(
        questions[0].other_answer_id.as_ref().unwrap().as_str(),
        "question_0_custom"
    );
    assert!(questions[1].multi_select);
    let content = |value| {
        let answers = answers(value);
        request.admit(&answers).unwrap();
        Value::Object(request.content(&answers))
    };
    assert_eq!(
        content(json!([
            {"questionId": "question_0", "text": "Only docs"},
            {"questionId": "question_1", "selectedOptions": ["Lint", "Tests"]}
        ])),
        json!({"question_0_custom": "Only docs", "question_1": ["lint", "test"]})
    );
    assert_eq!(content(json!([])), json!({}));
}

#[test]
fn mcp_fields_are_typed_by_their_schema() {
    // The MCP subset Codex 0.146 relays (`McpElicitationSchema`).
    let request = form_request(&json!({"type": "object", "required": ["email", "count"],
    "properties": {
        "email": {"type": "string", "title": "Email", "default": "a@b.c"},
        "count": {"type": "integer", "minimum": 1},
        "ratio": {"type": "number", "default": 0.5},
        "agree": {"type": "boolean", "title": "Agree?"},
        "tier": {"type": "string", "enum": ["free", "pro"], "enumNames": ["Free", "Pro"]},
        "tags": {"type": "array", "items": {"type": "string", "enum": ["x", "y"]}}
    }}))
    .unwrap();
    let by_id = |id: &str| {
        request
            .questions
            .as_slice()
            .iter()
            .find(|q| q.id.as_str() == id)
            .unwrap()
    };
    assert_eq!(
        by_id("email").initial_text.as_ref().unwrap().as_str(),
        "a@b.c"
    );
    assert!(!by_id("email").optional && by_id("agree").optional);
    let answered = answers(json!([
        {"questionId": "email", "text": "me@example.com"},
        {"questionId": "count", "text": " 3 "},
        {"questionId": "ratio", "text": "2.5"},
        {"questionId": "agree", "selectedOption": "Yes"},
        {"questionId": "tier", "selectedOption": "Pro"},
        {"questionId": "tags", "selectedOptions": ["y"]}
    ]));
    request.admit(&answered).unwrap();
    assert_eq!(
        Value::Object(request.content(&answered)),
        json!({"email": "me@example.com", "count": 3, "ratio": 2.5, "agree": true,
            "tier": "pro", "tags": ["y"]})
    );
    for (value, error) in [
        (
            json!([{"questionId": "email", "text": "x"}]),
            UserInputError::Unanswered,
        ),
        (
            json!([{"questionId": "email", "text": "x"}, {"questionId": "count", "text": "1.5"}]),
            UserInputError::NotANumber,
        ),
        (
            json!([{"questionId": "email", "text": "x"}, {"questionId": "count", "text": "0"}]),
            UserInputError::OutOfRange,
        ),
        (
            json!([{"questionId": "tags", "selectedOption": "x"}]),
            UserInputError::UnknownOption,
        ),
        (
            json!([{"questionId": "tier", "selectedOptions": ["Free"]}]),
            UserInputError::UnknownOption,
        ),
        (
            json!([{"questionId": "tags", "selectedOptions": ["x", "x"]}]),
            UserInputError::UnknownOption,
        ),
    ] {
        assert_eq!(request.admit(&answers(value)), Err(error));
    }
}

#[test]
fn forms_sage_cannot_show_exactly_are_refused() {
    let property = json!({"type": "string"});
    let many: Map<String, Value> = (0..=USER_INPUT_MAX_QUESTIONS)
        .map(|index| (format!("f{index}"), property.clone()))
        .collect();
    for schema in [
        json!({"type": "object", "properties": {"f": {"type": "object"}}}),
        json!({"type": "object", "properties": many}),
        json!({"type": "object", "properties": {"f": {"type": "string", "enum": ["a", "a"]}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "enum": []}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "enum": vec!["v"; 9]}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "enum": ["a\u{202e}b"]}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "format": "email"}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "format": "date"}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "pattern": "^a"}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "minLength": 4, "maxLength": 3}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "minLength": -1}}}),
        json!({"type": "object", "properties": {"f": {"type": "string", "maxLength": "3"}}}),
        json!({"type": "object", "properties": {"f": {"type": "number", "minimum": 2, "maximum": 1.5}}}),
        json!({"type": "object", "properties": {"f": {"type": "integer", "minimum": null}}}),
        json!({"type": "object", "properties": {"f": {"type": "array", "minItems": 3,
            "items": {"type": "string", "enum": ["a", "b"]}}}}),
    ] {
        assert_eq!(form_request(&schema), None, "{schema}");
    }
}

#[test]
fn schema_limits_are_carried_and_enforced() {
    let request = form_request(&json!({"type": "object", "properties": {
        "name": {"type": "string", "minLength": 2, "maxLength": 3},
        "count": {"type": "integer", "minimum": -2, "maximum": 5},
        "ratio": {"type": "number", "minimum": 0.5, "maximum": 1},
        "tags": {"type": "array", "minItems": 2, "maxItems": 2,
            "items": {"type": "string", "enum": ["x", "y", "z"]}},
        "plain": {"type": "string"}
    }}))
    .unwrap();
    let json = serde_json::to_value(&request).unwrap();
    let limits_of = |id: &str| {
        json["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == id)
            .unwrap()
            .get("limits")
            .cloned()
    };
    assert_eq!(
        limits_of("name"),
        Some(json!({"minLength": 2, "maxLength": 3}))
    );
    assert_eq!(
        limits_of("ratio"),
        Some(json!({"minimum": 0.5, "maximum": 1}))
    );
    assert_eq!(
        limits_of("tags"),
        Some(json!({"minItems": 2, "maxItems": 2}))
    );
    assert_eq!(limits_of("plain"), None);
    let admit = |id: &str, answer: Value| {
        let mut answer = answer;
        answer["questionId"] = id.into();
        request.admit(&answers(json!([answer])))
    };
    let text = |id: &str, text: &str| admit(id, json!({"text": text}));
    assert_eq!(text("name", "é漢"), Ok(()));
    assert_eq!(text("name", "a"), Err(UserInputError::TextLength));
    assert_eq!(text("name", "abcd"), Err(UserInputError::TextLength));
    for ok in ["-2", "5", "+5", " 0 "] {
        assert_eq!(text("count", ok), Ok(()), "{ok}");
    }
    assert_eq!(text("count", "6"), Err(UserInputError::OutOfRange));
    assert_eq!(text("count", "-3"), Err(UserInputError::OutOfRange));
    assert_eq!(text("ratio", "1.0"), Ok(()));
    assert_eq!(text("ratio", "5e-1"), Ok(()));
    assert_eq!(text("ratio", "0.4"), Err(UserInputError::OutOfRange));
    assert_eq!(text("ratio", "0x1"), Err(UserInputError::NotANumber));
    let tags = |labels: Value| admit("tags", json!({"selectedOptions": labels}));
    assert_eq!(tags(json!(["x", "y"])), Ok(()));
    assert_eq!(tags(json!(["x"])), Err(UserInputError::ItemCount));
    assert_eq!(tags(json!(["x", "y", "z"])), Err(UserInputError::ItemCount));
}

#[test]
fn a_property_without_a_title_is_headed_by_its_key() {
    let request = form_request(&json!({"type": "object", "properties": {
        "branch_name": {"type": "string", "description": "Which branch?"},
        "blank": {"type": "string", "title": "  "},
        "titled": {"type": "string", "title": "Title"}
    }}))
    .unwrap();
    let headers: Vec<_> = request
        .questions
        .as_slice()
        .iter()
        .map(|q| (q.id.as_str(), q.header.as_str()))
        .collect();
    assert!(headers.contains(&("branch_name", "branch_name")));
    assert!(headers.contains(&("blank", "blank")));
    assert!(headers.contains(&("titled", "Title")));
}

#[test]
fn a_custom_answer_field_with_its_own_limits_is_refused() {
    let mut schema = ask_user_question();
    schema["properties"]["question_0_custom"]["maxLength"] = json!(10);
    assert_eq!(form_request(&schema), None);
}

#[test]
fn links_are_shown_and_opened_normalized() {
    let request = url_request("HTTPS://Auth.Example.com/a b?q=é").unwrap();
    let link = request.url.as_ref().unwrap();
    assert_eq!(link.url.as_str(), "https://auth.example.com/a%20b?q=%C3%A9");
    assert_eq!(link.host.as_str(), "auth.example.com");
    for refused in [
        "https://example.com/\u{202e}gpj.exe",
        "https://exa\u{200b}mple.com/",
        "https://example.com/\u{2066}x",
        " https://example.com/",
        "https://example.com/ ",
        "https://example.com/\tx",
        "https://exam\nple.com/",
    ] {
        assert_eq!(url_request(refused), None, "{refused:?}");
    }
}

#[test]
fn links_are_admitted_only_for_web_hosts_without_credentials() {
    let request = url_request("https://auth.example.com/oauth?state=1").unwrap();
    let link = request.url.as_ref().unwrap();
    assert_eq!(link.host.as_str(), "auth.example.com");
    assert!(request.questions.as_slice().is_empty());
    request.admit(&answers(json!([]))).unwrap();
    assert_eq!(
        request.admit(&answers(json!([{"questionId": "x", "text": "y"}]))),
        Err(UserInputError::UnknownQuestion)
    );
    for refused in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://user:pw@example.com/",
        "not a url",
    ] {
        assert_eq!(url_request(refused), None, "{refused}");
    }
}

#[test]
fn question_payload_carries_the_clipped_harness_message() {
    let request = form_request(&ask_user_question()).unwrap();
    let message = "é".repeat(2000);
    let Some(NormalizedPayload::PermissionRequest { reason, .. }) =
        question_payload("Answer the agent's questions", Some(&message), request)
    else {
        panic!("payload");
    };
    let reason = reason.unwrap();
    assert!(reason.as_str().len() <= 2048 && reason.as_str().starts_with('é'));
}
