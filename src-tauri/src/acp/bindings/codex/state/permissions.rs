use crate::acp::{
    bindings::standard_acp::elicitation::accepted, intent::user_input::UserInputAnswers,
    lifecycle::ServerRequestState, limits::LIMITS, runtime::permissions::PermissionDecision,
};

/// The Codex server request that asks the user questions.
pub(in crate::acp::bindings::codex) const USER_INPUT_METHOD: &str = "item/tool/requestUserInput";

use super::{
    AdapterError, CodexAdapter, ExternalKind, PermissionRegistration, PermissionRequestEntry,
    PermissionRequestParams,
};

impl CodexAdapter {
    pub(crate) fn register_permission_request(
        &mut self,
        params: PermissionRequestParams<'_>,
    ) -> Result<PermissionRegistration, AdapterError> {
        let PermissionRequestParams {
            request_id,
            fingerprint,
            method,
            allow_always,
            turn_id,
            item_id,
            response,
            form,
        } = params;
        self.expect_turn(turn_id)?;
        if let Some(existing) = self
            .permission_requests
            .iter()
            .find(|entry| entry.id == request_id)
        {
            let allow_response_queued = existing.state == ServerRequestState::ResponseQueued
                && existing.response.is_empty();
            if existing.fingerprint != fingerprint
                || existing.method != method
                || existing.allow_always != allow_always
                || existing.turn_id != turn_id
                || existing.item_id != item_id
                || (!allow_response_queued
                    && !matches!(
                        existing.state,
                        ServerRequestState::AllowPending
                            | ServerRequestState::AllowCommitted
                            | ServerRequestState::ResolvedAllowed
                    )
                    && existing.response != response)
            {
                return Err(AdapterError::PermissionTeardownRequired);
            }
            return Ok(match existing.state {
                ServerRequestState::Received
                | ServerRequestState::Asked
                | ServerRequestState::AllowPending
                | ServerRequestState::AllowCommitted
                | ServerRequestState::ResolvedAllowed => PermissionRegistration::DuplicatePending,
                ServerRequestState::ResponseQueued if allow_response_queued => {
                    PermissionRegistration::DuplicatePending
                }
                ServerRequestState::DeclinePending => PermissionRegistration::DuplicatePending,
                ServerRequestState::DeclineCommitted
                | ServerRequestState::ResponseQueued
                | ServerRequestState::ResolvedDeclined => {
                    PermissionRegistration::DuplicateCommitted(existing.response.clone())
                }
                ServerRequestState::Cancelled => PermissionRegistration::DuplicatePending,
            });
        }
        if self.permission_requests.len() >= LIMITS.queues.server_requests_per_session as usize {
            let Some(index) = self.permission_requests.iter().position(|entry| {
                matches!(
                    entry.state,
                    ServerRequestState::ResolvedAllowed
                        | ServerRequestState::ResolvedDeclined
                        | ServerRequestState::Cancelled
                )
            }) else {
                return Err(AdapterError::PermissionTeardownRequired);
            };
            self.permission_requests.swap_remove(index);
        }
        let external = self.scoped_in_turn(ExternalKind::Request, turn_id, request_id)?;
        self.permission_requests.push(PermissionRequestEntry {
            id: request_id.to_owned(),
            fingerprint,
            method: method.to_owned(),
            allow_always,
            turn_id: turn_id.to_owned(),
            item_id: item_id.to_owned(),
            response: response.to_vec(),
            state: ServerRequestState::Received,
            form,
        });
        Ok(PermissionRegistration::New(external))
    }

    pub(crate) fn advance_permission(
        &mut self,
        request_id: &str,
        next: ServerRequestState,
    ) -> Result<(), AdapterError> {
        let entry = self
            .permission_requests
            .iter_mut()
            .find(|entry| entry.id == request_id)
            .ok_or(AdapterError::PermissionTeardownRequired)?;
        if entry.state == next {
            return Ok(());
        }
        if !entry.state.can_transition_to(next) {
            return Err(AdapterError::PermissionTeardownRequired);
        }
        entry.state = next;
        Ok(())
    }

    pub(crate) fn advance_permission_decline(
        &mut self,
        request_id: &str,
        next: ServerRequestState,
    ) -> Result<(), AdapterError> {
        self.advance_permission(request_id, next)
    }

    /// Returns the exact native app-server approval response retained at
    /// ingress. Command/file approvals decline; granular permission requests
    /// return an empty permission set. The separate turn/interrupt request is
    /// still emitted by `CodexAdapter::cancel`; ACP wire is never synthesized.
    pub(crate) fn cancelled_permission_response(
        &self,
        request_id: &str,
    ) -> Result<Vec<u8>, AdapterError> {
        let entry = self
            .permission_requests
            .iter()
            .find(|entry| entry.id == request_id)
            .ok_or(AdapterError::PermissionTeardownRequired)?;
        if !matches!(
            entry.state,
            ServerRequestState::Received | ServerRequestState::Asked
        ) {
            return Err(AdapterError::PermissionTeardownRequired);
        }
        Ok(entry.response.clone())
    }

    pub(crate) fn resolve_pending_permission(
        &mut self,
        request_id: &str,
        decision: &PermissionDecision,
    ) -> Result<Option<(String, Vec<u8>, String)>, AdapterError> {
        let Some(entry) = self
            .permission_requests
            .iter_mut()
            .find(|entry| entry.id == request_id)
        else {
            return Err(AdapterError::PermissionTeardownRequired);
        };
        if !matches!(
            entry.state,
            ServerRequestState::Received | ServerRequestState::Asked
        ) {
            return Err(AdapterError::PermissionTeardownRequired);
        }
        let asks_user = entry.method == USER_INPUT_METHOD || entry.form.is_some();
        match decision {
            PermissionDecision::Ask => {
                entry.state = ServerRequestState::Asked;
                Ok(None)
            }
            PermissionDecision::AllowOnce
            | PermissionDecision::AllowAlways
            | PermissionDecision::Answer(_) => {
                if (*decision == PermissionDecision::AllowAlways && !entry.allow_always)
                    || asks_user != matches!(decision, PermissionDecision::Answer(_))
                {
                    return Err(AdapterError::PermissionTeardownRequired);
                }
                entry.state = ServerRequestState::AllowPending;
                entry.state = ServerRequestState::AllowCommitted;
                let mut value: serde_json::Value = serde_json::from_slice(&entry.response)
                    .map_err(|_| AdapterError::PermissionTeardownRequired)?;
                let result = match decision {
                    PermissionDecision::Answer(answers) => match &entry.form {
                        Some(form) => accepted(form, answers),
                        None => user_input_result(answers),
                    },
                    _ if entry.method == "item/permissions/requestApproval" => {
                        serde_json::json!({"permissions": {}})
                    }
                    _ => {
                        serde_json::json!({"decision": if *decision == PermissionDecision::AllowAlways { "acceptForSession" } else { "accept" }})
                    }
                };
                value
                    .as_object_mut()
                    .ok_or(AdapterError::PermissionTeardownRequired)?
                    .insert("result".into(), result);
                let bytes = serde_json::to_vec(&value)
                    .map_err(|_| AdapterError::PermissionTeardownRequired)?;
                // An empty retained response marks the shared queued state as
                // the allow path, so a duplicate cannot replay decline bytes.
                entry.response.clear();
                Ok(Some((entry.id.clone(), bytes, entry.method.clone())))
            }
            PermissionDecision::Deny => {
                entry.state = ServerRequestState::DeclinePending;
                Ok(Some((
                    entry.id.clone(),
                    entry.response.clone(),
                    entry.method.clone(),
                )))
            }
        }
    }
}

/// Codex `ToolRequestUserInputResponse`, shaped as Codex's own client sends
/// it: the chosen option label, then any free text as a `user_note:` entry.
fn user_input_result(answers: &UserInputAnswers) -> serde_json::Value {
    let answers = answers
        .as_slice()
        .iter()
        .map(|answer| {
            let mut values = Vec::with_capacity(2);
            if let Some(label) = &answer.selected_option {
                values.push(label.as_str().to_owned());
            }
            if let Some(text) = &answer.text {
                values.push(format!("user_note: {}", text.as_str().trim()));
            }
            (
                answer.question_id.as_str().to_owned(),
                serde_json::json!({"answers": values}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({"answers": answers})
}
