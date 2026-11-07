use serde_json::json;

use super::*;

fn request() -> UserInputRequest {
    serde_json::from_value(json!({
        "questions": [
            {"id": "scope", "header": "Scope", "question": "Which files?",
             "options": [
                {"label": "All", "description": "Every file"},
                {"label": "Changed", "description": "Only changed files"}
             ]},
            {"id": "other", "header": "Name", "question": "Branch?", "allowsOther": true,
             "options": [{"label": "main", "description": ""}]},
            {"id": "free", "header": "Notes", "question": "Anything else?"},
            {"id": "token", "header": "Token", "question": "API token?", "secret": true}
        ],
        "autoResolutionMs": 90_000_000
    }))
    .unwrap()
}

fn answers(value: serde_json::Value) -> UserInputAnswers {
    serde_json::from_value(value).unwrap()
}

#[test]
fn answers_must_match_the_questions_that_were_asked() {
    let request = request();
    request
        .admit(&answers(json!([
            {"questionId": "scope", "selectedOption": "Changed"},
            {"questionId": "other", "text": "release/1.2"},
            {"questionId": "free", "text": "none"},
            {"questionId": "token", "text": "s3cret"}
        ])))
        .unwrap();
    for (value, error) in [
        (
            json!([{"questionId": "absent", "text": "x"}]),
            UserInputError::UnknownQuestion,
        ),
        (
            json!([{"questionId": "free", "text": "a"}, {"questionId": "free", "text": "b"}]),
            UserInputError::UnknownQuestion,
        ),
        (json!([{"questionId": "free"}]), UserInputError::EmptyAnswer),
        (
            json!([{"questionId": "scope", "selectedOption": "Some"}]),
            UserInputError::UnknownOption,
        ),
        (
            json!([{"questionId": "free", "selectedOption": "All"}]),
            UserInputError::UnknownOption,
        ),
        (
            json!([{"questionId": "scope", "text": "my own"}]),
            UserInputError::TextNotAdmitted,
        ),
    ] {
        assert_eq!(request.admit(&answers(value)), Err(error));
    }
}

#[test]
fn caps_are_enforced_when_a_request_or_answer_is_decoded() {
    let question = json!({"id": "q", "header": "", "question": "?"});
    assert!(
        serde_json::from_value::<UserInputRequest>(json!({"questions": vec![question; 5]}))
            .is_err()
    );
    let option = json!({"label": "a", "description": ""});
    assert!(
        serde_json::from_value::<UserInputRequest>(json!({"questions": [{
            "id": "q", "header": "", "question": "?", "options": vec![option; 9]
        }]}))
        .is_err()
    );
    assert!(
        serde_json::from_value::<UserInputAnswers>(json!([{
            "questionId": "q", "text": "x".repeat(4097)
        }]))
        .is_err()
    );
    assert_eq!(
        request().auto_resolution_window_ms(),
        Some(USER_INPUT_MAX_AUTO_RESOLUTION_MS)
    );
}

#[test]
fn debug_output_never_contains_answer_text() {
    let value = answers(json!([
        {"questionId": "token", "text": "s3cret-value"},
        {"questionId": "scope", "selectedOption": "Changed"}
    ]));
    let rendered = format!("{value:?}");
    assert!(rendered.contains("token"));
    assert!(!rendered.contains("s3cret-value"));
    assert!(!rendered.contains("Changed"));
}
