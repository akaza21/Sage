use serde_json::json;

use crate::acp::bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken};
use sage_acp_protocol::{contract::SageServerRequestId, permissions::PermissionDecision};

use super::super::{DenyIntent, DenyStage, PiRuntime};
use super::identity::external;
use crate::acp::bindings::adapter::AdapterEvent;
use crate::acp::bindings::pi::bridge::files::Prepared;
use crate::acp::bindings::pi::translator::ANSWER_ID;
use crate::acp::elicitation_form;
use sage_acp_protocol::{
    contract::{BoundedVec, SafeText},
    intent::{NormalizedPayload, ToolCategory},
};

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn resolve_later_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let Some(reply) = self.bridge_permission(request_id, &decision)? {
            self.questions.reply_pending = true;
            self.questions.settling = Some(request_id.as_str().to_owned());
            return Ok(reply);
        }
        let pending = self
            .deny
            .as_ref()
            .is_some_and(|intent| intent.request_id.as_str() == request_id.as_str());
        if !pending || !self.translator.permission_waiting_for(request_id.as_str()) {
            return Err(AdapterRuntimeError::LaterPermissionMiss);
        }
        if let Some(reply) = self.answer_question(&decision)? {
            self.questions.reply_pending = true;
            self.questions.visible = None;
            self.questions.settling = Some(request_id.as_str().to_owned());
            return crate::acp::runtime::later_permission::resolve_later_permission(
                request_id,
                &decision,
                Some(reply),
            )
            .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
            .ok_or(AdapterRuntimeError::LaterPermissionMiss);
        }
        let reply = match decision {
            PermissionDecision::Ask => {
                let _ = crate::acp::runtime::later_permission::resolve_later_permission(
                    request_id, &decision, None,
                )
                .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
                return Err(AdapterRuntimeError::Operation);
            }
            PermissionDecision::AllowOnce => {
                let pending = self
                    .translator
                    .take_pending_ui()
                    .ok_or(AdapterRuntimeError::LaterPermissionMiss)?;
                let body = match pending.method.as_str() {
                    "confirm" => serde_json::json!({
                        "type": "extension_ui_response",
                        "id": pending.id,
                        "confirmed": true,
                    }),
                    _ => {
                        self.translator.remember_pending_ui(pending);
                        return Err(AdapterRuntimeError::LaterPermissionMiss);
                    }
                };
                let bytes = serde_json::to_vec(&body).map_err(|_| AdapterRuntimeError::Frame)?;
                let frame = crate::acp::transport::OutboundFrame::new(bytes)
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                AdapterEgress::new(frame, EgressToken::protocol_notification())
            }
            PermissionDecision::AllowAlways | PermissionDecision::Answer(_) => {
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
            PermissionDecision::Deny
                if self
                    .translator
                    .pending_ui()
                    .is_some_and(|p| p.method == "confirm") =>
            {
                let pending = self
                    .translator
                    .take_pending_ui()
                    .ok_or(AdapterRuntimeError::LaterPermissionMiss)?;
                let frame = crate::acp::transport::OutboundFrame::new(
                    serde_json::to_vec(
                        &json!({"type":"extension_ui_response","id":pending.id,"cancelled":true}),
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                )
                .map_err(|_| AdapterRuntimeError::Frame)?;
                AdapterEgress::new(frame, EgressToken::protocol_notification())
            }
            PermissionDecision::Deny => self
                .mandatory_abort()?
                .ok_or(AdapterRuntimeError::Operation)?,
        };
        self.questions.reply_pending = true;
        self.questions.visible = None;
        self.questions.settling = Some(request_id.as_str().to_owned());
        crate::acp::runtime::later_permission::resolve_later_permission(
            request_id,
            &decision,
            Some(reply),
        )
        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
        .ok_or(AdapterRuntimeError::LaterPermissionMiss)
    }
}

impl PiRuntime {
    fn answer_question(
        &mut self,
        decision: &PermissionDecision,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let Some(pending) = self.translator.take_pending_ui() else {
            return Ok(None);
        };
        let Some(form) = pending
            .form
            .as_deref()
            .filter(|form| !form.is_confirmation())
        else {
            self.translator.remember_pending_ui(pending);
            return Ok(None);
        };
        let body = match decision {
            PermissionDecision::Answer(answers) => {
                if form.admit(answers).is_err() {
                    self.translator.remember_pending_ui(pending);
                    return Err(AdapterRuntimeError::UnsupportedCommand);
                }
                let mut content = form.content(answers);
                let Some(value) = content.remove(ANSWER_ID) else {
                    self.translator.remember_pending_ui(pending);
                    return Err(AdapterRuntimeError::UnsupportedCommand);
                };
                json!({"type": "extension_ui_response", "id": pending.id, "value": value})
            }
            PermissionDecision::Deny => {
                json!({"type": "extension_ui_response", "id": pending.id, "cancelled": true})
            }
            PermissionDecision::Ask
            | PermissionDecision::AllowOnce
            | PermissionDecision::AllowAlways => {
                self.translator.remember_pending_ui(pending);
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
        };
        let bytes = serde_json::to_vec(&body).map_err(|_| AdapterRuntimeError::Frame)?;
        let frame = crate::acp::transport::OutboundFrame::new(bytes)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        Ok(Some(AdapterEgress::new(
            frame,
            EgressToken::protocol_notification(),
        )))
    }

    pub(super) fn permission_event(
        &mut self,
        external_id: String,
        request_digest: [u8; 32],
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = external("piPermissionId", external_id)?;
        let pending = self
            .translator
            .pending_ui()
            .filter(|pending| pending.id == id.as_str())
            .ok_or(AdapterRuntimeError::Operation)?;
        let offered = pending.offered_option_kinds();
        let confirmation = pending
            .form
            .as_ref()
            .is_some_and(|form| form.is_confirmation());
        if pending.form.is_some() {
            self.questions.visible = Some(pending.id.clone());
        }
        let question = pending
            .form
            .as_deref()
            .map(|form| {
                let mut form = form.clone();
                form.auto_resolution_ms = pending.remaining_ms();
                elicitation_form::question_payload("Answer the Pi extension's question", None, form)
                    .map(|mut payload| {
                        if confirmation
                            && let NormalizedPayload::PermissionRequest {
                                offered_option_kinds,
                                ..
                            } = &mut payload
                        {
                            *offered_option_kinds = Some(
                                BoundedVec::new("permissionOptionKinds", offered.clone())
                                    .expect("two approval options"),
                            );
                        }
                        payload
                    })
                    .ok_or(AdapterRuntimeError::Frame)
            })
            .transpose()?;
        let turn_id = self.turn_identity()?;
        let item_id = self.identity(
            id.clone(),
            Some(turn_id.external_id.clone()),
            turn_id.turn_client_message_id.clone(),
        );
        self.deny = Some(DenyIntent {
            request_id: id.clone(),
            command_id: String::new(),
            stage: DenyStage::Prepared,
            releases: question.is_some(),
        });
        Ok(AdapterEvent::PermissionRequest {
            turn_id,
            item_id,
            external_request_id: id,
            payload: match question {
                Some(question) => question,
                None => NormalizedPayload::PermissionRequest {
                    user_input: None,
                    category: match self
                        .bridge
                        .as_ref()
                        .and_then(|b| b.pending.as_ref())
                        .map(|p| &p.prepared)
                    {
                        Some(Prepared::Edit(_)) => ToolCategory::Edit,
                        Some(Prepared::Shell(_)) => ToolCategory::Execute,
                        Some(Prepared::Extension(_)) => ToolCategory::Extension,
                        None => ToolCategory::Other,
                    },
                    // Prepare admits only paths and commands that fit and show exactly.
                    summary: SafeText::new(
                        "permissionSummary",
                        match self
                            .bridge
                            .as_ref()
                            .and_then(|b| b.pending.as_ref())
                            .map(|p| &p.prepared)
                        {
                            Some(Prepared::Edit(edit)) => {
                                format!("Allow Pi to update {}?", edit.relative)
                            }
                            Some(Prepared::Shell(shell)) => {
                                format!("Allow Pi to run this command?\n{}", shell.shown)
                            }
                            Some(Prepared::Extension(extension)) => {
                                extension.summary.as_str().to_owned()
                            }
                            None => "Pi extension gate is awaiting your decision".into(),
                        },
                    )
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                    offered_option_kinds: Some(
                        BoundedVec::new("permissionOptionKinds", offered)
                            .map_err(|_| AdapterRuntimeError::Frame)?,
                    ),
                    reason: None,
                },
            },
            request_payload_digest: request_digest,
        })
    }
}
