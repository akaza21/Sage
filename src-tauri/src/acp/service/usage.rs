use super::*;
use crate::acp::{
    goal::{NativeRpcIntent, NativeRpcRequest},
    runtime::HostError,
    scheduler::RuntimeOwner,
    usage::UsageState,
};

impl AcpService {
    /// Refreshes the runtime's usage limits from its harness, then answers
    /// with the latest snapshot. A refused or failed refresh still serves the
    /// snapshot the harness last pushed.
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
                | HostError::Deadline
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        cell.snapshot()
            .map_or(UsageState::Unavailable, |snapshot| UsageState::Usage {
                snapshot,
            })
    }
}
