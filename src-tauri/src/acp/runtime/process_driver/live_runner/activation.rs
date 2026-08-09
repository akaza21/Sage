use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeControl,
    runtime::{
        ActivationProcessDriver, ActivationTransport, CoordinatorActivationJournal, HostError,
    },
};

use super::WorkerState;
use crate::acp::runtime::process_driver::{DriverMailbox, FirstTurnWorkerState, StdinTransport};

pub(super) async fn complete<A, R, T>(
    mut driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    mailbox: &DriverMailbox,
) -> Result<WorkerState<A, R, T>, HostError>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    driver.begin().map_err(|_| HostError::Unavailable)?;
    loop {
        if driver.completion_ready(mailbox) {
            return driver
                .try_into_idle_parts(mailbox)
                .map(FirstTurnWorkerState::Idle)
                .map_err(|_| HostError::RecoveryRequired);
        }
        driver
            .run_next(mailbox)
            .await
            .map_err(|_| HostError::RecoveryRequired)?;
    }
}
