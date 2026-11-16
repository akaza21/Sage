use sage_acp_protocol::scheduler::RuntimeOwner;

use super::super::types::{HostError, HostOutcome, HostReply, HostState, RuntimeEngine};
use super::Actor;

impl<E: RuntimeEngine> Actor<E> {
    /// The worker opened a Turn the harness started on its own; it runs as
    /// the active Turn, so a stop or its terminal reaches it.
    pub(super) fn turn_started(
        &mut self,
        owner: RuntimeOwner,
        turn_id: sage_acp_protocol::contract::SageTurnId,
    ) -> Result<HostReply, HostError> {
        if self.state != HostState::Idle || !self.matches_owner(&owner) {
            return Err(HostError::Intent);
        }
        self.active_turn = Some(turn_id);
        self.state = HostState::Running;
        Ok(HostReply::Outcome(HostOutcome::TurnStarted))
    }
}
