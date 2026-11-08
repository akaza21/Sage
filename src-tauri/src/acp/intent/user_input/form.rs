//! Elicitation forms and links, shared by every harness that speaks them:
//! ACP `elicitation/create` (Claude Code) and MCP elicitation relayed by
//! Codex (`mcpServer/elicitation/request`). Both carry the MCP
//! `requestedSchema` subset: a flat object of string, enum, multi-select,
//! boolean, number and integer properties. A form Sage cannot show exactly
//! (an unknown property type, too many fields or options, repeated labels)
//! is refused here, so the caller declines it instead of guessing.
//!
//! The mapping follows the one in T3 Code's `elicitationForm.ts` (MIT).

use serde_json::{Map, Value};

use super::{
    USER_INPUT_MAX_OPTIONS, USER_INPUT_MAX_QUESTIONS, UserInputKind, UserInputOption,
    UserInputQuestion, UserInputRequest, UserInputUrl,
};
use crate::acp::{
    bindings::standard_acp::sanitize::normalized_label_text,
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{NormalizedPayload, PermissionOptionKind, ToolCategory},
    limits::LIMITS,
};

/// `_meta` marker the Claude adapter puts on the free-text companion of an
/// AskUserQuestion choice; it names the choice field it belongs to.
const CUSTOM_ANSWER_META: &str = "_askUserQuestionCustomAnswer";

/// A form elicitation as questions, or `None` when it cannot be shown.
pub(crate) fn form_request(schema: &Value) -> Option<UserInputRequest> {
    let schema = schema.as_object()?;
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|keys| keys.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut questions = Vec::new();
    let mut companions = Vec::new();
    for (key, property) in schema.get("properties")?.as_object()? {
        let property = property.as_object()?;
        if let Some(target) = companion_of(property) {
            companions.push((target, key.as_str()));
            continue;
        }
        if questions.len() == USER_INPUT_MAX_QUESTIONS {
            return None;
        }
        let mut question = field(key, property)?;
        question.optional = !required.contains(&key.as_str());
        questions.push(question);
    }
    for (target, key) in companions {
        let question = questions
            .iter_mut()
            .find(|question| question.id.as_str() == target && question.options.is_some())?;
        question.allows_other = true;
        question.other_answer_id = Some(RequiredText::new("userInputOtherId", key).ok()?);
    }
    Some(UserInputRequest {
        questions: BoundedVec::new("userInputQuestions", questions).ok()?,
        auto_resolution_ms: None,
        url: None,
    })
}

/// A link elicitation, admitted only for `http`/`https` with a host and no
/// embedded credentials.
pub(crate) fn url_request(url: &str) -> Option<UserInputRequest> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || url.chars().any(char::is_control)
    {
        return None;
    }
    Some(UserInputRequest {
        questions: BoundedVec::default(),
        auto_resolution_ms: None,
        url: Some(UserInputUrl {
            url: RequiredText::new("userInputUrl", url).ok()?,
            host: RequiredText::new("userInputUrlHost", host).ok()?,
        }),
    })
}

/// The pending-request payload for harness questions: a fixed `summary`, the
/// harness's own `message` (clipped) as the reason, and only a decline as
/// the approval choice. `None` when it would not fit the journal's cap.
pub(crate) fn question_payload(
    summary: &str,
    message: Option<&str>,
    request: UserInputRequest,
) -> Option<NormalizedPayload> {
    let reason = match message {
        Some(message) => Some(display::<2048>("permissionReason", message)?),
        None => None,
    }
    .filter(|reason| !reason.as_str().trim().is_empty());
    let payload = NormalizedPayload::PermissionRequest {
        category: ToolCategory::Other,
        summary: SafeText::new("permissionSummary", summary).ok()?,
        offered_option_kinds: Some(
            BoundedVec::new(
                "permissionOptionKinds",
                vec![PermissionOptionKind::RejectOnce],
            )
            .ok()?,
        ),
        reason,
        user_input: Some(Box::new(request)),
    };
    serde_json::to_vec(&payload)
        .is_ok_and(|json| json.len() <= LIMITS.wire.permission_payload_bytes as usize)
        .then_some(payload)
}

/// Harness display text, neutralized and redacted, clipped to `MAX` bytes.
pub(crate) fn display<const MAX: usize>(field: &'static str, text: &str) -> Option<SafeText<MAX>> {
    let mut end = text.len().min(MAX);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    normalized_label_text(field, &text[..end]).ok()
}

fn companion_of(property: &Map<String, Value>) -> Option<&str> {
    let marker = property.get("_meta")?.get(CUSTOM_ANSWER_META)?;
    (property.get("type")?.as_str()? == "string"
        && marker.get("isCustomAnswer").and_then(Value::as_bool) == Some(true))
    .then(|| marker.get("questionId")?.as_str())
    .flatten()
}

fn field(key: &str, property: &Map<String, Value>) -> Option<UserInputQuestion> {
    let text = |name| {
        property
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let mut question = UserInputQuestion::text(
        RequiredText::new("userInputQuestionId", key).ok()?,
        display("userInputHeader", text("title"))?,
        display("userInputQuestion", text("description"))?,
    );
    let initial = |value: &Value| match value {
        Value::String(text) => display("userInputInitialText", text),
        Value::Number(number) => display("userInputInitialText", &number.to_string()),
        _ => None,
    };
    match property.get("type")?.as_str()? {
        "string" => {
            let options = match (property.get("oneOf"), property.get("enum")) {
                (Some(titled), _) => Some(titled_options(titled)?),
                (None, Some(values)) => Some(plain_options(values, property.get("enumNames"))?),
                (None, None) => None,
            };
            if options.is_none() {
                question.initial_text = property.get("default").and_then(initial);
            }
            question.options = options;
        }
        "array" => {
            let items = property.get("items")?.as_object()?;
            question.options = Some(match items.get("anyOf").or_else(|| items.get("oneOf")) {
                Some(titled) => titled_options(titled)?,
                None => plain_options(items.get("enum")?, None)?,
            });
            question.multi_select = true;
        }
        "boolean" => {
            question.kind = UserInputKind::Boolean;
            question.options = Some(
                BoundedVec::new(
                    "userInputOptions",
                    vec![choice("Yes", "", "true")?, choice("No", "", "false")?],
                )
                .ok()?,
            );
        }
        kind @ ("number" | "integer") => {
            question.kind = if kind == "number" {
                UserInputKind::Number
            } else {
                UserInputKind::Integer
            };
            question.initial_text = property.get("default").and_then(initial);
        }
        _ => return None,
    }
    Some(question)
}

fn titled_options(entries: &Value) -> Option<BoundedVec<UserInputOption, USER_INPUT_MAX_OPTIONS>> {
    let options = entries
        .as_array()?
        .iter()
        .map(|entry| {
            let value = entry.get("const")?.as_str()?;
            let title = entry.get("title").and_then(Value::as_str).unwrap_or(value);
            let description = entry.get("description").and_then(Value::as_str);
            choice(title, description.unwrap_or_default(), value)
        })
        .collect::<Option<Vec<_>>>()?;
    distinct(options)
}

/// Options named by their values, checked as every harness's choices are.
pub(crate) fn plain_options(
    values: &Value,
    names: Option<&Value>,
) -> Option<BoundedVec<UserInputOption, USER_INPUT_MAX_OPTIONS>> {
    let values = values.as_array()?;
    let names = names.and_then(Value::as_array);
    let options = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value = value.as_str()?;
            let name = names.and_then(|names| names.get(index)?.as_str());
            choice(name.unwrap_or(value), "", value)
        })
        .collect::<Option<Vec<_>>>()?;
    distinct(options)
}

/// One option; the label is shown and echoed back, so it is checked rather
/// than rewritten, and the value is kept verbatim.
fn choice(label: &str, description: &str, value: &str) -> Option<UserInputOption> {
    let shown = display::<256>("userInputOptionLabel", label)?;
    if shown.as_str() != label || label.trim().is_empty() {
        return None;
    }
    Some(UserInputOption {
        label: RequiredText::new("userInputOptionLabel", label).ok()?,
        description: display("userInputOptionDescription", description)?,
        value: (value != label)
            .then(|| RequiredText::new("userInputOptionValue", value).ok())
            .flatten(),
    })
}

/// Answers name options by label, so a list that repeats one is refused.
fn distinct(
    options: Vec<UserInputOption>,
) -> Option<BoundedVec<UserInputOption, USER_INPUT_MAX_OPTIONS>> {
    let repeats = options.iter().enumerate().any(|(index, option)| {
        options[..index]
            .iter()
            .any(|earlier| earlier.label == option.label)
    });
    if options.is_empty() || repeats {
        return None;
    }
    BoundedVec::new("userInputOptions", options).ok()
}

#[cfg(test)]
mod tests;
