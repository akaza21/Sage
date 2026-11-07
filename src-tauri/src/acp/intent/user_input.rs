//! Harness-neutral "the harness asks the user" contract.
//!
//! A harness question rides the server-request pipeline beside approvals: it
//! is journaled as a `PermissionRequest` whose payload carries
//! `user_input`, so cancellation, turn interrupt and harness withdrawal reuse
//! the approval lifecycle. Answers never enter the journal, transcript or
//! diagnostics; they exist only in the in-memory decision and the reply
//! frame. Every field below is bounded; the caps are part of the contract.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::acp::contract::{BoundedVec, RequiredText, SafeText};

/// Most questions one request may ask.
pub const USER_INPUT_MAX_QUESTIONS: usize = 4;
/// Most selectable options one question may offer.
pub const USER_INPUT_MAX_OPTIONS: usize = 8;
/// Longest admitted auto-resolution window (24 hours); longer windows clamp.
pub const USER_INPUT_MAX_AUTO_RESOLUTION_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputOption {
    pub label: RequiredText<256>,
    pub description: SafeText<1024>,
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
}

impl UserInputQuestion {
    fn admits_text(&self) -> bool {
        self.options.is_none() || self.allows_other || self.secret
    }

    fn offers(&self, label: &str) -> bool {
        self.options.as_ref().is_some_and(|options| {
            options
                .as_slice()
                .iter()
                .any(|option| option.label.as_str() == label)
        })
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
}

/// One answered question. Unanswered questions are simply omitted.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputAnswer {
    pub question_id: RequiredText<128>,
    /// The label of the chosen option, exactly as offered.
    #[serde(default)]
    pub selected_option: Option<RequiredText<256>>,
    /// Free text: the answer to a free-text, "other" or secret question.
    #[serde(default)]
    pub text: Option<RequiredText<4096>>,
}

/// The user's answers to one request. Answer text never appears in `Debug`
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
    #[error("an answer is empty")]
    EmptyAnswer,
    #[error("an answer selects an option the question does not offer")]
    UnknownOption,
    #[error("an answer gives free text the question does not admit")]
    TextNotAdmitted,
}

impl UserInputAnswers {
    pub fn new(answers: Vec<UserInputAnswer>) -> Result<Self, crate::acp::contract::ContractError> {
        BoundedVec::new("userInputAnswers", answers).map(Self)
    }

    pub fn as_slice(&self) -> &[UserInputAnswer] {
        self.0.as_slice()
    }

    /// Bytes this value retains, for bounded mailbox accounting.
    pub fn retained_bytes(&self) -> usize {
        self.as_slice()
            .iter()
            .map(|answer| {
                answer.question_id.as_str().len()
                    + answer
                        .selected_option
                        .as_ref()
                        .map_or(0, |value| value.as_str().len())
                    + answer.text.as_ref().map_or(0, |value| value.as_str().len())
            })
            .sum()
    }
}

impl UserInputRequest {
    /// The auto-resolution window, clamped to the admitted maximum.
    pub fn auto_resolution_window_ms(&self) -> Option<u64> {
        self.auto_resolution_ms
            .map(|value| value.min(USER_INPUT_MAX_AUTO_RESOLUTION_MS))
    }

    /// Checks answers against exactly the questions this request asked.
    pub fn admit(&self, answers: &UserInputAnswers) -> Result<(), UserInputError> {
        let questions = self.questions.as_slice();
        for (index, answer) in answers.as_slice().iter().enumerate() {
            let id = answer.question_id.as_str();
            let question = questions
                .iter()
                .find(|question| question.id.as_str() == id)
                .ok_or(UserInputError::UnknownQuestion)?;
            if answers.as_slice()[..index]
                .iter()
                .any(|earlier| earlier.question_id.as_str() == id)
            {
                return Err(UserInputError::UnknownQuestion);
            }
            if answer.selected_option.is_none() && answer.text.is_none() {
                return Err(UserInputError::EmptyAnswer);
            }
            if let Some(label) = &answer.selected_option
                && !question.offers(label.as_str())
            {
                return Err(UserInputError::UnknownOption);
            }
            if answer.text.is_some() && !question.admits_text() {
                return Err(UserInputError::TextNotAdmitted);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
