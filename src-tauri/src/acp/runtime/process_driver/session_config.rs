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
}
