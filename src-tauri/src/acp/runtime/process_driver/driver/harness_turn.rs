use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::AdapterBatchJournal,
};

use super::ProcessAdapterDriver;
use crate::acp::runtime::process_driver::{
    DispatchSource, DriverError, DriverProgress, ProcessTreeControl, StdinTransport, TreeLeaseOwner,
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
    pub(crate) async fn adopt_started_turn(&mut self, now_millis: i64) -> Result<(), DriverError> {
        self.ensure_running()?;
        if let Err(error) = self.pump.adopt_started_turn(now_millis).await {
            return Err(self.pump_error(error));
        }
        let mut progress = DriverProgress::default();
        loop {
            self.flush_all_ingress(DispatchSource::Ingress, now_millis, &mut progress)
                .await?;
            match self.pump.drain_stdout_deferred(now_millis).await {
                Ok(true) => progress.accepted_frames += 1,
                Ok(false) => return Ok(()),
                Err(error) => return Err(self.pump_error(error)),
            }
        }
    }
}
