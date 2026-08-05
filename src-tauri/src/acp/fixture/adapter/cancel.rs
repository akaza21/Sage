use crate::acp::bindings::adapter::{
    AdapterEgress, AdapterRuntimeError, EgressResolution, EgressToken,
};

use super::{FixtureAdapter, State};

impl FixtureAdapter {
    pub(super) fn cancel_command(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.state != State::AwaitingTextStart {
            return Err(AdapterRuntimeError::Operation);
        }
        let request = self.request_with(
            br#"{"jsonrpc":"2.0","method":"fixture/cancel","params":{}}"#.to_vec(),
            EgressToken::cancel_request,
        )?;
        self.state = State::CancelCreated;
        Ok(request)
    }

    pub(super) fn resolve_cancel_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if token.request_id() != self.pending_request_id {
            return Err(AdapterRuntimeError::EgressIntent);
        }
        let next = match (self.state, resolution) {
            (State::CancelCreated, EgressResolution::Queued) => State::CancelQueued,
            (State::CancelCreated, EgressResolution::QueueRejected)
            | (State::CancelQueued, EgressResolution::WriteZeroBytes)
            | (State::CancelQueued, EgressResolution::Written) => State::AwaitingTextStart,
            (State::CancelQueued, EgressResolution::DeliveryUnknown) => State::Complete,
            _ => return Err(AdapterRuntimeError::EgressIntent),
        };
        if resolution != EgressResolution::Queued {
            self.pending_request_id = None;
        }
        self.state = next;
        Ok(())
    }
}
