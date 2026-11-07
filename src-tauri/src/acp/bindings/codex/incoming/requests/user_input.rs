//! `item/tool/requestUserInput` (Codex `ToolRequestUserInputParams`) mapped
//! onto the harness-neutral `UserInputRequest`.

use serde_json::{Map, Value, json};

use crate::acp::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        NormalizedPayload, PermissionOptionKind, ToolCategory,
        user_input::{UserInputOption, UserInputQuestion, UserInputRequest},
    },
    limits::LIMITS,
};

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
    if questions.is_empty() {
        return None;
    }
    let auto_resolution_ms = match params.get("autoResolutionMs") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64()?),
    };
    Some(UserInputRequest {
        questions: BoundedVec::new("userInputQuestions", questions).ok()?,
        auto_resolution_ms,
    })
}

fn question(value: &Value) -> Option<UserInputQuestion> {
    let value = value.as_object()?;
    let options = match value.get("options") {
        None | Some(Value::Null) => None,
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
        id: RequiredText::new("userInputQuestionId", string(value, "id")?).ok()?,
        header: SafeText::new("userInputHeader", sanitize(string(value, "header")?)).ok()?,
        question: SafeText::new("userInputQuestion", sanitize(string(value, "question")?)).ok()?,
        options,
        allows_other: flag(value, "isOther")?,
        secret: flag(value, "isSecret")?,
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
