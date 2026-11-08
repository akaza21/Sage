//! Claude's `AskUserQuestion` tool as a Sage question card. Its approval
//! request carries the questions; the user's answers return as the tool's
//! input with an `answers` map from question text to the chosen label or
//! typed text. Declining answers nothing, so Claude carries on without.

use serde_json::{Map, Value, json};

use crate::acp::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::user_input::{
        USER_INPUT_MAX_OPTIONS, USER_INPUT_MAX_QUESTIONS, UserInputOption, UserInputQuestion,
        UserInputRequest,
    },
};

use super::notices::prose;

/// The questions as Sage shows them, and each question's text in order
/// (answers are keyed by it). `None` when they cannot be shown.
pub(super) fn questions(input: &Value) -> Option<(UserInputRequest, Vec<String>)> {
    let asked = input.get("questions")?.as_array()?;
    if asked.is_empty() || asked.len() > USER_INPUT_MAX_QUESTIONS {
        return None;
    }
    let mut texts = Vec::with_capacity(asked.len());
    let mut shown = Vec::with_capacity(asked.len());
    for (index, question) in asked.iter().enumerate() {
        let text = question.get("question")?.as_str()?;
        let header = question.get("header").and_then(Value::as_str).unwrap_or("");
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .filter(|options| !options.is_empty() && options.len() <= USER_INPUT_MAX_OPTIONS)
            .map(|options| options.iter().map(option).collect::<Option<Vec<_>>>())
            .unwrap_or(Some(Vec::new()))?;
        shown.push(UserInputQuestion {
            id: RequiredText::new("userInputQuestionId", format!("q{index}")).ok()?,
            header: SafeText::new("userInputHeader", prose(header, 256).unwrap_or_default())
                .ok()?,
            question: SafeText::new("userInputQuestion", prose(text, 2048)?).ok()?,
            options: (!options.is_empty())
                .then(|| BoundedVec::new("userInputOptions", options).ok())
                .flatten(),
            // Claude Code always offers its own "Other" answer.
            allows_other: true,
            multi_select: question.get("multiSelect").and_then(Value::as_bool) == Some(true),
            secret: false,
            kind: Default::default(),
            optional: false,
            initial_text: None,
            other_answer_id: None,
            limits: Default::default(),
        });
        texts.push(text.to_owned());
    }
    let request = UserInputRequest {
        questions: BoundedVec::new("userInputQuestions", shown).ok()?,
        auto_resolution_ms: None,
        url: None,
    };
    Some((request, texts))
}

fn option(value: &Value) -> Option<UserInputOption> {
    let label = value.get("label")?.as_str()?;
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(UserInputOption {
        // The label is sent back as the answer, so it must stay exact.
        label: RequiredText::new("userInputOptionLabel", label.to_owned()).ok()?,
        description: SafeText::new(
            "userInputOptionDescription",
            prose(description, 1024).unwrap_or_default(),
        )
        .ok()?,
        value: None,
    })
}

/// The tool's input with the answers Sage collected: `{questionId,
/// selectedOption?, text?}` entries, read back by question order.
pub(super) fn answered(input: &Value, texts: &[String], answers: Option<&Value>) -> Value {
    let mut map = Map::new();
    for answer in answers.and_then(Value::as_array).into_iter().flatten() {
        let index = answer
            .get("questionId")
            .and_then(Value::as_str)
            .and_then(|id| id.strip_prefix('q'))
            .and_then(|index| index.parse::<usize>().ok());
        let Some(text) = index.and_then(|index| texts.get(index)) else {
            continue;
        };
        // Claude Code reads several chosen labels as one comma-separated answer.
        let picked = answer
            .get("selectedOptions")
            .and_then(Value::as_array)
            .map(|picked| {
                picked
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|picked| !picked.is_empty())
            .or_else(|| {
                answer
                    .get("selectedOption")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        let typed = answer.get("text").and_then(Value::as_str).map(str::trim);
        let reply = match (picked, typed.filter(|typed| !typed.is_empty())) {
            (Some(picked), Some(typed)) => format!("{picked}, {typed}"),
            (Some(picked), None) => picked,
            (None, Some(typed)) => typed.to_owned(),
            (None, None) => continue,
        };
        map.insert(text.clone(), reply.into());
    }
    let mut input = input.clone();
    if let Some(object) = input.as_object_mut() {
        object.insert("answers".into(), Value::Object(map));
    } else {
        input = json!({"answers": map});
    }
    input
}
