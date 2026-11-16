use crate::acp::bindings::adapter::{AdapterRuntimeError, EgressResolution, EgressToken};
use sage_acp_protocol::lifecycle::ServerRequestState;

use super::super::CodexRuntime;

impl CodexRuntime {
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
