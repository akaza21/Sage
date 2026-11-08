//! Question requests with an auto-resolution window resolve themselves
//! (without answers) when the window passes; the driver's deadline loop
//! wakes for the earliest window and sends the decline.

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::AdapterBatchJournal,
};

use super::ProcessAdapterDriver;
use crate::acp::runtime::process_driver::{
    DispatchSource, DriverError, DriverProgress, ProcessTreeControl, StdinTransport, TeardownCause,
    TreeLeaseOwner,
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
    /// Sends the decline for a question request whose window has passed,
    /// exactly as a user cancel would.
    pub(super) async fn auto_resolve_due(
        &mut self,
        now_millis: i64,
        progress: &mut DriverProgress,
    ) -> Result<(), DriverError> {
        match self.pump.auto_resolve_due(now_millis).await {
            Ok(false) => return Ok(()),
            Ok(true) => {}
            Err(error) => return Err(self.pump_error(error)),
        }
        self.flush_all_ingress(DispatchSource::Ingress, now_millis, progress)
            .await?;
        if progress.zero_byte_failures != 0 {
            return Err(self.latch(TeardownCause::MandatoryDenyFailure));
        }
        Ok(())
    }
}
