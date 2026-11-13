//! ACP `elicitation/create` for any standard ACP agent (schema 1.4, the
//! unstable elicitation capability): the agent asks the user to fill a form
//! (`mode: "form"`, an MCP-style `requestedSchema`) or to open a link
//! (`mode: "url"`, typically an MCP sign-in). The request becomes the shared
//! harness question request; the reply is `{action: "accept", content}`,
//! `decline` (the user refused) or `cancel` (nothing could ask, or the Turn
//! ended under it). A runtime keeps the returned form beside its pending
//! request so it can type the accepted content.

use serde_json::{Value, json};

use crate::acp::intent::{
    NormalizedPayload,
    user_input::{UserInputAnswers, UserInputRequest, form},
};

pub(crate) const ELICITATION_CREATE: &str = "elicitation/create";

/// The `clientCapabilities.elicitation` a runtime advertises once it routes
/// `elicitation/create` here.
pub(crate) fn client_capability() -> Value {
    json!({"form": {}, "url": {}})
}

/// What to do with one `elicitation/create` request.
#[derive(Debug, PartialEq)]
pub(crate) enum Elicitation {
    /// Show the card; keep `form` to type the answer.
    Ask {
        payload: Box<NormalizedPayload>,
        form: Box<UserInputRequest>,
    },
    /// Reply at once with this result.
    Reply(Value),
}

/// Admits the request's `params` for the session `session_id`. Only a
/// session-scoped request inside a running Turn can be asked; a form or
/// link Sage cannot show exactly is declined.
pub(crate) fn admit(params: &Value, session_id: &str, turn_running: bool) -> Elicitation {
    if !turn_running || params.get("sessionId").and_then(Value::as_str) != Some(session_id) {
        return Elicitation::Reply(refused(false));
    }
    let request = match params.get("mode").and_then(Value::as_str) {
        Some("form") => params.get("requestedSchema").and_then(form::form_request),
        Some("url") => params
            .get("url")
            .and_then(Value::as_str)
            .and_then(form::url_request),
        _ => None,
    };
    let message = params.get("message").and_then(Value::as_str);
    let asked = request.and_then(|request| {
        let summary = if request.url.is_some() {
            "Open a link to continue"
        } else {
            "Answer the agent's questions"
        };
        let payload = form::question_payload(summary, message, request.clone())?;
        Some(Elicitation::Ask {
            payload: Box::new(payload),
            form: Box::new(request),
        })
    });
    asked.unwrap_or_else(|| Elicitation::Reply(refused(true)))
}

/// The result for admitted answers: link requests carry no content.
pub(crate) fn accepted(form: &UserInputRequest, answers: &UserInputAnswers) -> Value {
    if form.url.is_some() {
        json!({"action": "accept"})
    } else {
        json!({"action": "accept", "content": form.content(answers)})
    }
}

/// The result when the user declines (`declined`) or nothing answered.
pub(crate) fn refused(declined: bool) -> Value {
    json!({"action": if declined { "decline" } else { "cancel" }})
}

#[cfg(test)]
mod tests;
