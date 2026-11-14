use super::*;
use crate::acp::{
    goal::{NativeRpcIntent, NativeRpcRequest},
    projection::AcpChatPage,
    runtime::HostError,
    scheduler::RuntimeOwner,
    usage::{UsageCell, UsageState},
};

impl AcpService {
    /// A read that misses its deadline is abandoned and still serves the last
    /// snapshot.
    pub(crate) fn read_usage(&self, owner: RuntimeOwner) -> UsageState {
        let Some(lane) = self.lane_for(&owner) else {
            return UsageState::Offline;
        };
        let Ok(host) = lane.runtime.for_owner(&owner) else {
            return UsageState::Offline;
        };
        let Some(cell) = lane.runtime.usage.cell(&owner) else {
            return UsageState::Unsupported;
        };
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::UsageRead,
        });
        if matches!(
            result,
            Err(HostError::Unavailable
                | HostError::Closed
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        cell.state()
    }

    pub(super) fn compose_usage(&self, page: &mut AcpChatPage) {
        page.usage = self
            .current_usage_cell(page)
            .is_some_and(|cell| cell.supported());
    }

    fn current_usage_cell(&self, page: &AcpChatPage) -> Option<std::sync::Arc<UsageCell>> {
        let session = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| Some(&session.id) == page.current_session_id.as_ref())?;
        let owner = RuntimeOwner {
            project_id: page.project_id.clone(),
            chat_id: page.chat_id.clone(),
            session_id: session.id.clone(),
            runtime_epoch: session.runtime_epoch?,
        };
        self.lane_for(&owner)?.runtime.usage.cell(&owner)
    }
}
