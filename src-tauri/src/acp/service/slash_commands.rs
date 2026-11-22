use super::*;
impl AcpService {
    pub(crate) fn refresh_changed_catalogs(
        &self,
        before: &[sage_domain::harness::HarnessDefinition],
        after: &[sage_domain::harness::HarnessDefinition],
    ) {
        for lane in self.lanes() {
            let Some(owner) = lane.runtime.owner() else {
                continue;
            };
            let binding: Option<String> = self.database.connection().query_row("SELECT binding_id FROM agent_sessions WHERE id=?1 AND project_id=?2 AND chat_id=?3", rusqlite::params![owner.session_id.as_str(),owner.project_id.as_str(),owner.chat_id.as_str()], |row| row.get(0)).ok();
            let Some(record) = binding
                .as_deref()
                .and_then(|binding| self.registry.route(binding).ok())
            else {
                continue;
            };
            let id = record.harness_id();
            let previous = before
                .iter()
                .find(|harness| harness.id == id)
                .and_then(|harness| harness.version.as_deref());
            let next = after
                .iter()
                .find(|harness| harness.id == id)
                .and_then(|harness| harness.version.as_deref());
            if next.is_some() && next != previous {
                let _ = self.refresh_commands(owner);
            }
        }
    }
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
