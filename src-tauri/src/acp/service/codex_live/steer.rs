use crate::acp::{
    intent::BindingId,
    runtime::{HostOutcome, HostSteerIntent},
};

use super::live_recovery::LiveHostRecoveryPhase;
use super::{CodexSessionError, map_slot_error};
use crate::acp::service::SessionDispatchError;
use crate::acp::service::{AcpService, live_lanes::Lane};

impl AcpService {
    pub(crate) fn dispatch_session_steer(
        &self,
        binding_id: &BindingId,
        intent: HostSteerIntent,
    ) -> Result<HostOutcome, SessionDispatchError> {
        let binding = self
            .resolve_session_binding(binding_id)
            .map_err(|_| SessionDispatchError::Binding)?;
        let capabilities = binding
            .supported_capabilities()
            .map_err(|_| SessionDispatchError::Binding)?;
        if !capabilities.active_turn_steer {
            return Err(SessionDispatchError::Binding);
        }
        self.lane_for(&intent.owner)
            .ok_or(CodexSessionError::Intent)
            .and_then(|lane| lane.steer_codex_turn(intent))
            .map_err(SessionDispatchError::Operation)
    }
}

impl Lane<'_> {
    fn steer_codex_turn(&self, intent: HostSteerIntent) -> Result<HostOutcome, CodexSessionError> {
        let owner = intent.owner.clone();
        let host = self.runtime.for_owner(&owner).map_err(map_slot_error)?;
        host.steer(intent).map_err(|error| {
            self.retain_live_host_error(owner, host, LiveHostRecoveryPhase::SendEffect, error)
        })
    }
}
