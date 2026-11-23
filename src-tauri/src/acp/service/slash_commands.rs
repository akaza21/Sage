use super::*;
impl AcpService {
    pub(crate) fn refresh_commands(
        &self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let lane = self
            .lane_for(&owner)
            .ok_or(crate::acp::runtime::HostError::Unavailable)?;
        let host = lane
            .runtime
            .for_owner(&owner)
            .map_err(|_| crate::acp::runtime::HostError::Unavailable)?;
        host.native_rpc(crate::acp::goal::NativeRpcIntent {
            owner,
            request: crate::acp::goal::NativeRpcRequest::RefreshCommands,
        })?;
        Ok(())
    }
    pub(crate) fn set_commands_notice_sink(
        &self,
        sink: crate::acp::slash_commands::CommandNoticeSink,
    ) {
        for lane in self.lanes() {
            lane.runtime.commands.set_sink(sink.clone());
        }
    }
    pub(crate) fn available_commands(
        &self,
        owner: &sage_acp_protocol::scheduler::RuntimeOwner,
    ) -> Option<std::sync::Arc<crate::acp::slash_commands::CommandState>> {
        let lane = self.lane_for(owner)?;
        lane.runtime.for_owner(owner).ok()?;
        lane.runtime.commands.snapshot(owner)
    }
}
