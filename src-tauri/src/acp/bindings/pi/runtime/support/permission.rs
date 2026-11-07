use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken},
    contract::SageServerRequestId,
    runtime::permissions::PermissionDecision,
};

use super::super::PiRuntime;
use crate::acp::bindings::pi::protocol::is_allow_option;

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn resolve_later_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
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
            PermissionDecision::AllowAlways | PermissionDecision::Answer(_) => {
                return Err(AdapterRuntimeError::UnsupportedCommand);
            }
            PermissionDecision::Deny => self
                .mandatory_abort()?
                .ok_or(AdapterRuntimeError::Operation)?,
        };
        crate::acp::runtime::later_permission::resolve_later_permission(
            request_id,
            &decision,
            Some(reply),
        )
        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
        .ok_or(AdapterRuntimeError::LaterPermissionMiss)
    }
}
