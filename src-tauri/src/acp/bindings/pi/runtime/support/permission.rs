use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken},
    contract::SageServerRequestId,
    limits::LIMITS,
    runtime::permissions::PermissionDecision,
    transport::OutboundFrame,
};

use super::super::{DenyStage, PiRuntime};
use crate::acp::bindings::pi::protocol::is_allow_option;

impl PiRuntime {
    pub(super) fn cancel_dialog(
        &mut self,
        id: String,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.cancelled_dialogs.len() >= LIMITS.queues.outstanding_rpc_per_session as usize
            || self.cancelled_dialogs.contains_key(&id)
        {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let external = super::identity::external("piDialogId", id.clone())?;
        let bytes = serde_json::to_vec(&serde_json::json!({
            "type":"extension_ui_response", "id":id, "cancelled":true,
        }))
        .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        let frame =
            OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
        self.cancelled_dialogs.insert(id, DenyStage::Prepared);
        Ok(AdapterEgress::new(
            frame,
            EgressToken::permission_cancel(external),
        ))
    }

    pub(in crate::acp::bindings::pi::runtime) fn commit_dialog_cancel(
        &mut self,
        token: &EgressToken,
    ) -> Result<(), AdapterRuntimeError> {
        let id = token
            .cancelled_permission_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        let stage = self
            .cancelled_dialogs
            .get_mut(id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if *stage == DenyStage::Prepared {
            *stage = DenyStage::Committed;
        }
        Ok(())
    }

    pub(in crate::acp::bindings::pi::runtime) fn resolve_dialog_cancel(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        let id = token
            .cancelled_permission_request()
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            self.poison();
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let stage = self
            .cancelled_dialogs
            .get_mut(id)
            .ok_or(AdapterRuntimeError::EgressIntent)?;
        match (*stage, resolution) {
            (DenyStage::Committed, EgressResolution::Queued) => *stage = DenyStage::Queued,
            (DenyStage::Queued, EgressResolution::Written) => {
                self.cancelled_dialogs.remove(id);
            }
            (DenyStage::Queued, EgressResolution::Queued) => {}
            _ => return Err(AdapterRuntimeError::EgressIntent),
        }
        Ok(())
    }

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
