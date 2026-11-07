use super::*;
impl AcpService {
    pub(crate) fn available_commands(
        &self,
        owner: &crate::acp::scheduler::RuntimeOwner,
    ) -> Option<std::sync::Arc<crate::acp::slash_commands::CommandState>> {
        let lane = self.lane_for(owner)?;
        lane.runtime.for_owner(owner).ok()?;
        lane.runtime.commands.snapshot(owner)
    }
}
