use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken},
    runtime::later_permission::resolve_later_permission,
    transport::OutboundFrame,
};
use sage_acp_protocol::{
    contract::SageServerRequestId, lifecycle::ServerRequestState, permissions::PermissionDecision,
};

use super::super::CodexRuntime;

impl CodexRuntime {
    /// Sage's later answer to a permission request Codex still waits on.
    pub(super) fn resolve_later_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: &PermissionDecision,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let pending = self
            .inner
            .resolve_pending_permission(request_id.as_str(), decision)
            .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?;
        match pending {
            Some((_, bytes, _)) if *decision != PermissionDecision::Ask => {
                let frame = OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?;
                resolve_later_permission(
                    request_id,
                    decision,
                    Some(AdapterEgress::new(
                        frame,
                        EgressToken::protocol_notification(),
                    )),
                )
                .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)?
                .ok_or(AdapterRuntimeError::LaterPermissionMiss)
            }
            _ => Err(AdapterRuntimeError::Operation),
        }
    }

    /// Records how Sage's answer to a permission request left the process:
    /// an allow, a decline, or the cancel of a request the turn outlived.
    pub(super) fn resolve_permission_egress(
        &mut self,
        token: &EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if let Some(request_id) = token.allow_request() {
            let next = match resolution {
                EgressResolution::Queued => ServerRequestState::ResponseQueued,
                EgressResolution::Written => ServerRequestState::ResolvedAllowed,
                EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown => {
                    return Err(AdapterRuntimeError::MandatoryDenyTeardown);
                }
            };
            return self
                .inner
                .advance_permission(request_id, next)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if let Some(request_id) = token.cancelled_permission_request() {
            if matches!(
                resolution,
                EgressResolution::QueueRejected
                    | EgressResolution::WriteZeroBytes
                    | EgressResolution::DeliveryUnknown
            ) {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            return self
                .inner
                .advance_permission(request_id, ServerRequestState::Cancelled)
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let Some((request_id, replay)) = token.deny_request() else {
            return Err(AdapterRuntimeError::EgressIntent);
        };
        if matches!(
            resolution,
            EgressResolution::QueueRejected
                | EgressResolution::WriteZeroBytes
                | EgressResolution::DeliveryUnknown
        ) {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        if replay {
            return Ok(());
        }
        let next = match resolution {
            EgressResolution::Queued => ServerRequestState::ResponseQueued,
            EgressResolution::Written => ServerRequestState::ResolvedDeclined,
            EgressResolution::QueueRejected
            | EgressResolution::WriteZeroBytes
            | EgressResolution::DeliveryUnknown => unreachable!(),
        };
        self.inner
            .advance_permission(request_id, next)
            .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)
    }
}
