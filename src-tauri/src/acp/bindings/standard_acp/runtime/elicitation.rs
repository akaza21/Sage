//! Routes ACP `elicitation/create` into the shared question card. An asked
//! request is published as a permission request carrying the form; the
//! user's answers reply `accept`, Deny replies `decline`, and a cancelled
//! Turn replies `cancel`. Every other request (outside a running Turn, for
//! another session, or a form Sage cannot show) is answered at once.

use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterEgress, AdapterEvent, AdapterRuntimeError, EgressResolution,
        EgressToken,
    },
    intent::{ExternalId, user_input::UserInputRequest},
    runtime::permissions::PermissionDecision,
};

use super::super::elicitation::{self as codec, Elicitation};
use super::{StandardAcpRuntime, outbound, permission_identity, scoped_permission_id};

/// Elicitations remembered at once; a finished one is retired for a new one.
const MAX_ELICITATIONS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    Pending,
    Accepted,
    Declined,
    Cancelled,
}

pub(super) struct ElicitationIntent {
    wire_id: Value,
    request_digest: [u8; 32],
    form: Box<UserInputRequest>,
    reply: Reply,
    written: bool,
}

impl StandardAcpRuntime {
    pub(super) fn is_elicitation(&self, value: &Value) -> bool {
        self.profile.elicitation
            && value.get("method").and_then(Value::as_str) == Some(codec::ELICITATION_CREATE)
    }

    pub(super) fn ingest_elicitation(
        &mut self,
        value: Value,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let (wire_token, _, wire_id) = permission_identity(value.get("id"))?;
        let request_id = scoped_permission_id(&self.session_id, self.runtime_epoch, &wire_token)?;
        if let Some(existing) = self.elicitations.get(request_id.as_str()) {
            if existing.request_digest != request_digest {
                return Err(AdapterRuntimeError::Frame);
            }
            return AdapterBatch::new(vec![AdapterEvent::IgnoredDuplicate], Vec::new());
        }
        let session = self
            .active
            .as_ref()
            .map(|active| active.external_id.clone());
        let turn_running = self
            .active
            .as_ref()
            .is_some_and(|active| active.turn.is_some())
            && !self.close_pending
            && !self.safety_alerted;
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let admitted = codec::admit(
            &params,
            session.as_ref().map_or("", ExternalId::as_str),
            turn_running,
        );
        let (payload, form) = match admitted {
            Elicitation::Reply(result) => return reply_now(&wire_id, result),
            Elicitation::Ask { payload, form } => (payload, form),
        };
        if self.elicitations.len() >= MAX_ELICITATIONS {
            let retired = self
                .elicitations
                .iter()
                .find(|(_, intent)| intent.written)
                .map(|(key, _)| key.clone());
            match retired {
                Some(key) => {
                    self.elicitations.remove(&key);
                }
                None => return reply_now(&wire_id, codec::refused(false)),
            }
        }
        let event = AdapterEvent::PermissionRequest {
            turn_id: self.turn_identity()?,
            item_id: self.item_identity(request_id.clone())?,
            external_request_id: request_id.clone(),
            payload: *payload,
            request_payload_digest: request_digest,
        };
        self.elicitations.insert(
            request_id.as_str().to_owned(),
            ElicitationIntent {
                wire_id,
                request_digest,
                form,
                reply: Reply::Pending,
                written: false,
            },
        );
        AdapterBatch::new(vec![event], Vec::new())
    }

    /// Answers a pending elicitation with the user's decision, or `None`
    /// when `request_id` names no elicitation.
    pub(super) fn resolve_elicitation(
        &mut self,
        request_id: &str,
        decision: &PermissionDecision,
    ) -> Option<Result<AdapterEgress, AdapterRuntimeError>> {
        let intent = self.elicitations.get_mut(request_id)?;
        if intent.reply != Reply::Pending {
            return Some(Err(AdapterRuntimeError::LaterPermissionMiss));
        }
        let (reply, result) = match decision {
            PermissionDecision::Answer(answers) => {
                if intent.form.admit(answers).is_err() {
                    return Some(Err(AdapterRuntimeError::UnsupportedCommand));
                }
                (Reply::Accepted, codec::accepted(&intent.form, answers))
            }
            PermissionDecision::Deny => (Reply::Declined, codec::refused(true)),
            PermissionDecision::AllowOnce | PermissionDecision::AllowAlways => {
                return Some(Err(AdapterRuntimeError::UnsupportedCommand));
            }
            PermissionDecision::Ask => return Some(Err(AdapterRuntimeError::Operation)),
        };
        Some(egress(request_id, intent, reply, result))
    }

    /// The Turn was cancelled under a pending elicitation.
    pub(super) fn cancel_elicitation(
        &mut self,
        request_id: &str,
    ) -> Option<Result<AdapterEgress, AdapterRuntimeError>> {
        let intent = self.elicitations.get_mut(request_id)?;
        if intent.reply != Reply::Pending {
            return Some(Err(AdapterRuntimeError::PermissionIntent));
        }
        Some(egress(
            request_id,
            intent,
            Reply::Cancelled,
            codec::refused(false),
        ))
    }

    /// Settles the egress of an elicitation reply; `None` when `request_id`
    /// names no elicitation.
    pub(super) fn settle_elicitation(
        &mut self,
        request_id: &str,
        resolution: Option<EgressResolution>,
    ) -> Option<Result<(), AdapterRuntimeError>> {
        let intent = self.elicitations.get_mut(request_id)?;
        if intent.reply == Reply::Pending {
            return Some(Err(AdapterRuntimeError::EgressIntent));
        }
        match resolution {
            None | Some(EgressResolution::Queued) => {}
            Some(EgressResolution::Written) => intent.written = true,
            Some(_) => return Some(Err(AdapterRuntimeError::MandatoryDenyTeardown)),
        }
        Some(Ok(()))
    }
}

fn egress(
    request_id: &str,
    intent: &mut ElicitationIntent,
    reply: Reply,
    result: Value,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    let external = ExternalId::new("permissionRequest", request_id)
        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
    let token = match reply {
        Reply::Accepted => EgressToken::permission_allow(external),
        Reply::Declined => EgressToken::mandatory_deny(external, false),
        Reply::Cancelled | Reply::Pending => EgressToken::permission_cancel(external),
    };
    let frame = outbound(json!({"jsonrpc": "2.0", "id": intent.wire_id, "result": result}))?;
    intent.reply = reply;
    Ok(AdapterEgress::new(frame, token))
}

fn reply_now(wire_id: &Value, result: Value) -> Result<AdapterBatch, AdapterRuntimeError> {
    AdapterBatch::new(
        Vec::new(),
        vec![AdapterEgress::new(
            outbound(json!({"jsonrpc": "2.0", "id": wire_id, "result": result}))?,
            EgressToken::client_fs_response(),
        )],
    )
}
