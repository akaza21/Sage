//! Undo holds the same lane claim as activation and idle/replacement parking.
use super::*;
use crate::acp::service::SessionControlError;

impl AcpService {
    pub(in crate::acp::service) fn claim_undo_lane(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<ActivationClaim<'_>, SessionControlError> {
        let lane = self
            .lanes
            .iter()
            .find(|lane| lane.holds(owner))
            .ok_or(SessionControlError::Unavailable)?;
        lane.activating
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| SessionControlError::Unavailable)?;
        let claim = ActivationClaim { lane };
        lane.runtime
            .for_owner(owner)
            .map_err(|_| SessionControlError::Unavailable)?;
        if lane
            .scheduler
            .active()
            .is_none_or(|active| &active.tree != owner || active.turn_reserved)
        {
            return Err(SessionControlError::Unavailable);
        }
        Ok(claim)
    }
}
