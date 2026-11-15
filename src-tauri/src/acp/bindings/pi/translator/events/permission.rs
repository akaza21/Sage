use super::super::{ANSWER_ID, PendingUiRequest, PiTranslator, TurnPhase};
use crate::acp::bindings::pi::{
    AdapterError, AdapterOutput, decode::external_id, protocol::is_allow_option,
};
use crate::acp::elicitation_form;
use sage_acp_protocol::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        session_event::SessionNoticeLevel,
        user_input::{UserInputKind, UserInputOption, UserInputQuestion, UserInputRequest},
    },
};
use serde_json::Value;

impl PiTranslator {
    pub(super) fn extension_request(
        &mut self,
        value: &Value,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.authority_denied && turn.lifecycle.terminal().is_none())
        {
            return Ok(vec![AdapterOutput::LateIgnored]);
        }
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if method == "notify" {
            let level = match value.get("notifyType").and_then(Value::as_str) {
                Some("error") => SessionNoticeLevel::Error,
                Some("warning") => SessionNoticeLevel::Warning,
                _ => SessionNoticeLevel::Info,
            };
            return Ok(vec![super::notice(level, "Pi", value.get("message"))]);
        }
        if matches!(
            method,
            "setStatus" | "setWidget" | "setTitle" | "set_editor_text"
        ) {
            let live = self
                .active
                .as_ref()
                .is_some_and(|turn| turn.lifecycle.terminal().is_none() && !turn.authority_denied);
            return Ok(vec![if live {
                AdapterOutput::Absorbed
            } else {
                AdapterOutput::LateIgnored
            }]);
        }
        if !matches!(method, "confirm" | "select" | "input" | "editor") {
            if self
                .active
                .as_ref()
                .is_none_or(|turn| turn.lifecycle.terminal().is_some() || turn.authority_denied)
            {
                return Ok(vec![AdapterOutput::LateIgnored]);
            }
            self.live_turn()?;
            return self.unknown_event("extension_ui_request");
        }
        let id = external_id(value.get("id"))?.ok_or(AdapterError::Malformed)?;
        let running = self.active.as_ref().is_some_and(|turn| {
            turn.lifecycle.phase == TurnPhase::ModelRunning
                && turn.lifecycle.terminal().is_none()
                && !turn.authority_denied
        });
        // One card at a time: Pi may run tools in parallel, and a second
        // dialog must not take the place of one the user is still answering.
        // A dialog with a timeout may already be gone, so a newer one replaces it.
        let open = self
            .pending_ui
            .as_ref()
            .is_some_and(|pending| !pending.expires);
        if !running || open {
            return Ok(vec![AdapterOutput::DialogCancelled { external_id: id }]);
        }
        let options = if method == "select" {
            let values = value
                .get("options")
                .and_then(Value::as_array)
                .ok_or(AdapterError::Malformed)?;
            if values.len() > 8 {
                return Err(AdapterError::Limit("extension options"));
            }
            values
                .iter()
                .map(|value| {
                    let option = value.as_str().ok_or(AdapterError::Malformed)?;
                    if option.len() > 64 || option.contains('\0') {
                        return Err(AdapterError::Limit("extension option bytes"));
                    }
                    Ok(option.to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        // Confirm, and a select offering an allow answer, gate Pi's work; every
        // other dialog is a question the user answers on the question card.
        let gate = method == "confirm" || options.iter().any(|option| is_allow_option(option));
        let form = if gate {
            None
        } else {
            match question(method, value) {
                Some(form) => Some(Box::new(form)),
                None => return Ok(vec![AdapterOutput::DialogCancelled { external_id: id }]),
            }
        };
        let expired = self.pending_ui.take().map(|pending| pending.id);
        self.remember_pending_ui(PendingUiRequest {
            id: id.clone(),
            method: method.to_owned(),
            options,
            form,
            expires: value
                .get("timeout")
                .is_some_and(|timeout| !timeout.is_null()),
        });
        let requested = AdapterOutput::PermissionRequested { external_id: id };
        Ok(match expired {
            Some(external_id) => vec![AdapterOutput::DialogExpired { external_id }, requested],
            None => vec![requested],
        })
    }
}

/// The dialog as one question, answered under `ANSWER_ID`; `None` when it
/// cannot be shown exactly.
fn question(method: &str, value: &Value) -> Option<UserInputRequest> {
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())?;
    let mut question = UserInputQuestion::text(
        RequiredText::new("userInputQuestionId", ANSWER_ID).ok()?,
        elicitation_form::display("userInputHeader", "Pi extension")?,
        elicitation_form::display("userInputQuestion", title)?,
    );
    match method {
        "input" => {}
        "editor" => {
            question.kind = UserInputKind::Multiline;
            question.initial_text = match value.get("prefill").and_then(Value::as_str) {
                Some(text) if !text.is_empty() => {
                    Some(elicitation_form::display("userInputInitialText", text)?)
                }
                _ => None,
            };
        }
        // A label sanitising would change, or a repeated one, cannot be
        // echoed back exactly, so such a select is cancelled.
        "select" => {
            question.options = Some(elicitation_form::plain_options(
                value.get("options")?,
                None,
            )?)
        }
        _ => return None,
    }
    Some(UserInputRequest {
        questions: BoundedVec::new("userInputQuestions", vec![question]).ok()?,
        auto_resolution_ms: None,
        url: None,
    })
}
