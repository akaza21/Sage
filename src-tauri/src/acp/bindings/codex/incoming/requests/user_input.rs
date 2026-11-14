//! `item/tool/requestUserInput` (Codex `ToolRequestUserInputParams`) mapped
//! onto the harness-neutral `UserInputRequest`.

use serde_json::{Map, Value, json};

use crate::acp::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        NormalizedPayload, PermissionOptionKind, ToolCategory,
        user_input::{UserInputOption, UserInputQuestion, UserInputRequest},
    },
};
use sage_core::limits::LIMITS;

use super::super::{
    IncomingEvent, IngressDisposition, IngressError, record_unprojected,
    support::{identity, sanitize, text},
};
use super::{CodexAdapter, PendingServerRequest, USER_INPUT_METHOD, project_server_request, reply};

pub(super) fn request(
    adapter: &mut CodexAdapter,
    object: &Map<String, Value>,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let id = object.get("id").ok_or(IngressError::Malformed)?;
    let thread = text(params, "threadId", 512)?;
    let turn = text(params, "turnId", 512)?;
    let item = text(params, "itemId", 512)?;
    adapter.expect_thread(thread).map_err(identity)?;
    adapter.expect_turn(turn).map_err(identity)?;
    // Empty answers are the protocol's decline: Codex's own client sends
    // them when the user dismisses the questions or they auto-resolve.
    let bytes = reply(id, "result", json!({"answers": {}}))?;
    let payload = parse(params).map(|request| NormalizedPayload::PermissionRequest {
        category: ToolCategory::Other,
        summary: SafeText::new("permissionSummary", "Answer the agent's questions")
            .expect("fixed summary"),
        offered_option_kinds: Some(
            BoundedVec::new(
                "permissionOptionKinds",
                vec![PermissionOptionKind::RejectOnce],
            )
            .expect("fixed permission choices"),
        ),
        reason: None,
        user_input: Some(Box::new(request)),
    });
    let Some(payload) = payload.filter(|payload| {
        serde_json::to_vec(payload)
            .is_ok_and(|json| json.len() <= LIMITS.wire.permission_payload_bytes as usize)
    }) else {
        // Questions beyond the contract's caps cannot be shown, so they are
        // declined at once rather than left waiting or torn down.
        record_unprojected(USER_INPUT_METHOD);
        return Ok(IncomingEvent::HostToolReply { bytes }.into());
    };
    project_server_request(
        adapter,
        PendingServerRequest {
            object,
            method: USER_INPUT_METHOD,
            allow_always: false,
            turn,
            item,
            bytes,
            payload,
            form: None,
        },
    )
}

fn parse(params: &Map<String, Value>) -> Option<UserInputRequest> {
    let questions = params
        .get("questions")?
        .as_array()?
        .iter()
        .map(question)
        .collect::<Option<Vec<_>>>()?;
    // Answers are keyed by question id, so a request that repeats one could
    // never be answered.
    let repeats_an_id = questions.iter().enumerate().any(|(index, question)| {
        questions[..index]
            .iter()
            .any(|earlier| earlier.id.as_str() == question.id.as_str())
    });
    if questions.is_empty() || repeats_an_id {
        return None;
    }
    // `isBlocking` only tells Codex's own client whether the turn waits on
    // the answer. Sage keeps every request open until the user answers or
    // Codex resolves it (`serverRequest/resolved`), so it is checked, not used.
    if !params
        .get("isBlocking")
        .is_none_or(|value| value.is_null() || value.is_boolean())
    {
        return None;
    }
    // `autoResolutionMs` is deprecated in the pinned schema; an explicit
    // window is still honoured when Codex sends one.
    let auto_resolution_ms = match params.get("autoResolutionMs") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64()?),
    };
    Some(UserInputRequest {
        questions: BoundedVec::new("userInputQuestions", questions).ok()?,
        auto_resolution_ms,
        url: None,
    })
}

fn question(value: &Value) -> Option<UserInputQuestion> {
    let value = value.as_object()?;
    // An empty option list offers nothing to pick, so the question takes
    // free text, as one without options does.
    let options = match value.get("options") {
        None | Some(Value::Null) => None,
        Some(Value::Array(options)) if options.is_empty() => None,
        Some(options) => Some(
            BoundedVec::new(
                "userInputOptions",
                options
                    .as_array()?
                    .iter()
                    .map(option)
                    .collect::<Option<Vec<_>>>()?,
            )
            .ok()?,
        ),
    };
    Some(UserInputQuestion {
        options,
        allows_other: flag(value, "isOther")?,
        secret: flag(value, "isSecret")?,
        ..UserInputQuestion::text(
            RequiredText::new("userInputQuestionId", string(value, "id")?).ok()?,
            SafeText::new("userInputHeader", sanitize(string(value, "header")?)).ok()?,
            SafeText::new("userInputQuestion", sanitize(string(value, "question")?)).ok()?,
        )
    })
}

fn option(value: &Value) -> Option<UserInputOption> {
    let value = value.as_object()?;
    // Labels are echoed back verbatim as answers, so they are not rewritten.
    Some(UserInputOption {
        label: RequiredText::new("userInputOptionLabel", string(value, "label")?).ok()?,
        description: SafeText::new(
            "userInputOptionDescription",
            sanitize(string(value, "description")?),
        )
        .ok()?,
        value: None,
    })
}

fn string<'a>(value: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn flag(value: &Map<String, Value>, key: &str) -> Option<bool> {
    match value.get(key) {
        None | Some(Value::Null) => Some(false),
        Some(value) => value.as_bool(),
    }
}
