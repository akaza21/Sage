use super::{ProcessAdapterDriver, ProcessTreeControl, StdinTransport, TreeLeaseOwner};
use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{AdapterBatchJournal, HostError},
    session_config::ConfigIntent,
};

impl<A, J, R, T, C, L> ProcessAdapterDriver<A, J, R, T, C, L>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    L: TreeLeaseOwner,
{
    pub(crate) fn begin_configure_session(
        &mut self,
        intent: &ConfigIntent,
    ) -> Result<(), HostError> {
        if intent.owner != self.owner {
            return Err(HostError::Intent);
        }
        if self.pump.config_pending() {
            return Err(HostError::State);
        }
        self.pump.write_session_config(&mut self.transport, intent)
    }

    pub(crate) fn config_pending(&self) -> bool {
        self.pump.config_pending()
    }
    pub(crate) fn goal_pending(&self) -> bool {
        self.pump.goal_pending()
    }
    pub(crate) fn native_rpc_pending(&self) -> bool {
        self.pump.native_rpc_pending()
    }
    pub(crate) fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.pump.take_native_rpc_reply()
    }
    pub(crate) fn begin_native_rpc(
        &mut self,
        intent: &crate::acp::goal::NativeRpcIntent,
    ) -> Result<(), HostError> {
        if intent.owner != self.owner || self.pump.native_rpc_pending() {
            return Err(HostError::Intent);
        }
        match &intent.request {
            crate::acp::goal::NativeRpcRequest::Goal(action) => {
                self.pump.write_goal(&mut self.transport, *action)
            }
            crate::acp::goal::NativeRpcRequest::ChildThreadRead {
                child_thread_id,
                parent_thread_id,
            } => self
                .pump
                .write_child(&mut self.transport, child_thread_id, parent_thread_id),
        }
    }
}
