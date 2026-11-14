//! A harness question rides the server-request pipeline beside approvals: it
//! is journaled as a `PermissionRequest` whose payload carries
//! `user_input`, so cancellation, turn interrupt and harness withdrawal reuse
//! the approval lifecycle. Answers never enter the journal, transcript or
//! diagnostics; they exist only in the in-memory decision and the reply
//! frame.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{BoundedVec, RequiredText, SafeText};

mod limits;

pub use limits::UserInputLimits;

pub const USER_INPUT_MAX_QUESTIONS: usize = 8;
pub const USER_INPUT_MAX_OPTIONS: usize = 8;
/// Longest answer text, and longest prefilled text, in UTF-8 bytes.
pub const USER_INPUT_TEXT_BYTES: usize = 16_384;
/// Longest admitted auto-resolution window (24 hours); longer windows clamp.
pub const USER_INPUT_MAX_AUTO_RESOLUTION_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputOption {
    pub label: RequiredText<256>,
    pub description: SafeText<1024>,
    /// The value the harness expects back, when it differs from the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<RequiredText<256>>,
}

impl UserInputOption {
    pub fn value(&self) -> &str {
        self.value.as_ref().unwrap_or(&self.label).as_str()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UserInputKind {
    /// One line of text, or a choice sent as its option value.
    #[default]
    Text,
    Multiline,
    Number,
    Integer,
    /// Yes or no: the options' values are `true` and `false`.
    Boolean,
}

impl UserInputKind {
    fn is_text(&self) -> bool {
        *self == Self::Text
    }
}

/// Only `http` and `https` links with a host and no credentials are admitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputUrl {
    pub url: RequiredText<2048>,
    /// The host, shown so the user sees where the link leads.
    pub host: RequiredText<256>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputQuestion {
    pub id: RequiredText<128>,
    pub header: SafeText<256>,
    pub question: SafeText<2048>,
    /// Absent means a free-text question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<BoundedVec<UserInputOption, USER_INPUT_MAX_OPTIONS>>,
    /// Free text is admitted beside (or instead of) a listed option.
    #[serde(default)]
    pub allows_other: bool,
    /// The answer is a secret: render it masked and never retain it.
    #[serde(default)]
    pub secret: bool,
    #[serde(default, skip_serializing_if = "UserInputKind::is_text")]
    pub kind: UserInputKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi_select: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_text: Option<SafeText<USER_INPUT_TEXT_BYTES>>,
    /// Free text is sent under this field id instead of the question's own,
    /// for forms that pair a choice with a separate "other" field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_answer_id: Option<RequiredText<128>>,
    #[serde(default, skip_serializing_if = "UserInputLimits::is_unset")]
    pub limits: UserInputLimits,
}

impl UserInputQuestion {
    pub fn text(id: RequiredText<128>, header: SafeText<256>, question: SafeText<2048>) -> Self {
        Self {
            id,
            header,
            question,
            options: None,
            allows_other: false,
            secret: false,
            kind: UserInputKind::Text,
            multi_select: false,
            optional: false,
            initial_text: None,
            other_answer_id: None,
            limits: UserInputLimits::default(),
        }
    }

    fn admits_text(&self) -> bool {
        self.options.is_none() || self.allows_other || self.secret
    }

    fn option(&self, label: &str) -> Option<&UserInputOption> {
        self.options.as_ref().and_then(|options| {
            options
                .as_slice()
                .iter()
                .find(|option| option.label.as_str() == label)
        })
    }

    fn admit(&self, answer: &UserInputAnswer) -> Result<(), UserInputError> {
        let labels = answer
            .selected_options
            .as_ref()
            .map_or(&[][..], BoundedVec::as_slice);
        let blank_text = answer
            .text
            .as_ref()
            .is_some_and(|text| text.as_str().trim().is_empty());
        if blank_text
            || (answer.selected_option.is_none() && labels.is_empty() && answer.text.is_none())
        {
            return Err(UserInputError::EmptyAnswer);
        }
        if let Some(label) = &answer.selected_option
            && (self.multi_select || self.option(label.as_str()).is_none())
        {
            return Err(UserInputError::UnknownOption);
        }
        let repeats = labels
            .iter()
            .enumerate()
            .any(|(index, label)| labels[..index].contains(label));
        if answer.selected_options.is_some()
            && (!self.multi_select
                || repeats
                || labels
                    .iter()
                    .any(|label| self.option(label.as_str()).is_none()))
        {
            return Err(UserInputError::UnknownOption);
        }
        if self.multi_select {
            self.limits.check_items(labels.len())?;
        }
        if let Some(text) = &answer.text {
            if !self.admits_text() {
                return Err(UserInputError::TextNotAdmitted);
            }
            // Text sent under another field id answers that field instead.
            if self.other_answer_id.is_none() {
                self.limits.check_text(self.kind, text.as_str())?;
            }
        }
        Ok(())
    }

    fn value_of<'a>(&'a self, label: &'a str) -> &'a str {
        self.option(label).map_or(label, UserInputOption::value)
    }

    /// `raw` typed by this question's kind; a number that does not parse
    /// stays a string (and is refused at admission).
    fn typed(&self, raw: &str) -> Value {
        let trimmed = raw.trim();
        match self.kind {
            UserInputKind::Boolean => Value::Bool(raw == "true"),
            UserInputKind::Integer => {
                limits::parse_integer(trimmed).map_or_else(|| raw.into(), Value::from)
            }
            UserInputKind::Number => limits::parse_number(trimmed)
                .and_then(serde_json::Number::from_f64)
                .map_or_else(|| raw.into(), Value::Number),
            UserInputKind::Text | UserInputKind::Multiline => raw.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputRequest {
    pub questions: BoundedVec<UserInputQuestion, USER_INPUT_MAX_QUESTIONS>,
    /// When present, the request resolves itself (with no answers) once this
    /// many milliseconds pass after it is received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resolution_ms: Option<u64>,
    /// Present when the request asks the user to open a link; it then has
    /// no questions and is answered, with no answers, once the user agrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<UserInputUrl>,
}

/// Unanswered questions are omitted.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputAnswer {
    pub question_id: RequiredText<128>,
    /// The label of the chosen option, exactly as offered.
    #[serde(default)]
    pub selected_option: Option<RequiredText<256>>,
    /// The labels chosen for a multi-select question.
    #[serde(default)]
    pub selected_options: Option<BoundedVec<RequiredText<256>, USER_INPUT_MAX_OPTIONS>>,
    /// Free text: the answer to a free-text, "other" or secret question.
    #[serde(default)]
    pub text: Option<RequiredText<USER_INPUT_TEXT_BYTES>>,
}

/// Answer text never appears in `Debug`
/// output, so it cannot leak through logs or error reports.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct UserInputAnswers(BoundedVec<UserInputAnswer, USER_INPUT_MAX_QUESTIONS>);

impl fmt::Debug for UserInputAnswer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserInputAnswer")
            .field("question_id", &self.question_id.as_str())
            .field("selected_option", &self.selected_option.is_some())
            .field("selected_options", &self.selected_options.is_some())
            .field("text", &self.text.is_some())
            .finish()
    }
}

impl fmt::Debug for UserInputAnswers {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.0.as_slice()).finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UserInputError {
    #[error("an answer names a question that was not asked, or names it twice")]
    UnknownQuestion,
    #[error("an answer is empty or only whitespace")]
    EmptyAnswer,
    #[error("an answer selects an option the question does not offer")]
    UnknownOption,
    #[error("an answer gives free text the question does not admit")]
    TextNotAdmitted,
    #[error("an answer to a number question is not a number of that kind")]
    NotANumber,
    #[error("an answer's text is shorter or longer than the question admits")]
    TextLength,
    #[error("a number answer is outside the question's range")]
    OutOfRange,
    #[error("an answer chooses fewer or more options than the question admits")]
    ItemCount,
    #[error("a required question is unanswered")]
    Unanswered,
}

impl UserInputAnswers {
    pub fn new(answers: Vec<UserInputAnswer>) -> Result<Self, crate::contract::ContractError> {
        BoundedVec::new("userInputAnswers", answers).map(Self)
    }

    pub fn as_slice(&self) -> &[UserInputAnswer] {
        self.0.as_slice()
    }

    pub fn retained_bytes(&self) -> usize {
        self.as_slice()
            .iter()
            .map(|answer| {
                let chosen = answer.selected_options.as_ref().map_or(0, |labels| {
                    labels
                        .as_slice()
                        .iter()
                        .map(|label| label.as_str().len())
                        .sum()
                });
                answer.question_id.as_str().len()
                    + answer
                        .selected_option
                        .as_ref()
                        .map_or(0, |value| value.as_str().len())
                    + chosen
                    + answer.text.as_ref().map_or(0, |value| value.as_str().len())
            })
            .sum()
    }
}

impl UserInputRequest {
    pub fn auto_resolution_window_ms(&self) -> Option<u64> {
        self.auto_resolution_ms
            .map(|value| value.min(USER_INPUT_MAX_AUTO_RESOLUTION_MS))
    }

    pub fn admit(&self, answers: &UserInputAnswers) -> Result<(), UserInputError> {
        let questions = self.questions.as_slice();
        let answers = answers.as_slice();
        for (index, answer) in answers.iter().enumerate() {
            let id = answer.question_id.as_str();
            let question = questions
                .iter()
                .find(|question| question.id.as_str() == id)
                .ok_or(UserInputError::UnknownQuestion)?;
            if answers[..index]
                .iter()
                .any(|earlier| earlier.question_id.as_str() == id)
            {
                return Err(UserInputError::UnknownQuestion);
            }
            question.admit(answer)?;
        }
        let unanswered = questions.iter().any(|question| {
            !question.optional
                && !answers
                    .iter()
                    .any(|answer| answer.question_id.as_str() == question.id.as_str())
        });
        if unanswered {
            return Err(UserInputError::Unanswered);
        }
        Ok(())
    }

    pub fn content(&self, answers: &UserInputAnswers) -> serde_json::Map<String, Value> {
        let mut content = serde_json::Map::new();
        for answer in answers.as_slice() {
            let Some(question) = self
                .questions
                .as_slice()
                .iter()
                .find(|question| question.id.as_str() == answer.question_id.as_str())
            else {
                continue;
            };
            let id = question.id.as_str();
            if let Some(label) = &answer.selected_option {
                content.insert(
                    id.to_owned(),
                    question.typed(question.value_of(label.as_str())),
                );
            }
            if let Some(labels) = &answer.selected_options {
                let values = labels.as_slice().iter();
                let values = values.map(|label| Value::from(question.value_of(label.as_str())));
                content.insert(id.to_owned(), Value::Array(values.collect()));
            }
            if let Some(text) = &answer.text {
                match &question.other_answer_id {
                    Some(other) => content.insert(other.as_str().to_owned(), text.as_str().into()),
                    None => content.insert(id.to_owned(), question.typed(text.as_str())),
                };
            }
        }
        content
    }
}

#[cfg(test)]
mod tests;
