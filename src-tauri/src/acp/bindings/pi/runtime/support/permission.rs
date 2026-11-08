use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken},
    contract::SageServerRequestId,
    runtime::permissions::PermissionDecision,
};

use super::super::{DenyIntent, DenyStage, PiRuntime};
use super::identity::external;
use crate::acp::bindings::pi::{protocol::is_allow_option, translator::PendingUiRequest};
use crate::acp::{
    bindings::adapter::AdapterEvent,
    contract::{BoundedVec, SafeText},
    intent::{NormalizedPayload, ToolCategory},
};

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn resolve_later_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let Some(reply) = self.bridge_permission(request_id, decision)? {
            return Ok(reply);
        }
        let pending = self
            .deny
            .as_ref()
            .is_some_and(|intent| intent.request_id.as_str() == request_id.as_str());
        if !pending || !self.translator.permission_waiting_for(request_id.as_str()) {
            return Err(AdapterRuntimeError::LaterPermissionMiss);
        }
        let reply = match decision {
            PermissionDecision::Ask => {
                let _ = crate::acp::runtime::later_permission::resolve_later_permission(
                    request_id, decision, None,
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
                    "select" => {
                        let Some(value) = pending
                            .options
                            .iter()
                            .find(|option| is_allow_option(option))
                        else {
                            self.translator.remember_pending_ui(pending);
                            return Err(AdapterRuntimeError::LaterPermissionMiss);
                        };
                        serde_json::json!({
                            "type": "extension_ui_response",
                            "id": pending.id,
                            "value": value,
                        })
                    }
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
            PermissionDecision::AllowAlways => {
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
            PermissionDecision::Deny => self
                .mandatory_abort()?
                .ok_or(AdapterRuntimeError::Operation)?,
        };
        crate::acp::runtime::later_permission::resolve_later_permission(
            request_id,
            decision,
            Some(reply),
        )
        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
        .ok_or(AdapterRuntimeError::LaterPermissionMiss)
    }
}

impl PiRuntime {
    pub(super) fn permission_event(
        &mut self,
        external_id: String,
        request_digest: [u8; 32],
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = external("piPermissionId", external_id)?;
        let offered = self
            .translator
            .pending_ui()
            .filter(|pending| pending.id == id.as_str())
            .map(PendingUiRequest::offered_option_kinds)
            .ok_or(AdapterRuntimeError::Operation)?;
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
        });
        Ok(AdapterEvent::PermissionRequest {
            turn_id,
            item_id,
            external_request_id: id,
            payload: NormalizedPayload::PermissionRequest {
                category: if self.bridge.as_ref().is_some_and(|b| b.pending.is_some()) {
                    ToolCategory::Edit
                } else {
                    ToolCategory::Other
                },
                summary: SafeText::new(
                    "permissionSummary",
                    self.bridge
                        .as_ref()
                        .and_then(|b| b.pending.as_ref())
                        // Prepare admits only paths that fit and show exactly.
                        .map(|p| format!("Allow Pi to update {}?", p.edit.relative))
                        .unwrap_or_else(|| "Pi extension gate is awaiting your decision".into()),
                )
                .map_err(|_| AdapterRuntimeError::Frame)?,
                offered_option_kinds: Some(
                    BoundedVec::new("permissionOptionKinds", offered)
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                ),
                reason: None,
            },
            request_payload_digest: request_digest,
        })
    }
}
