use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeControl,
    runtime::{
        ActivationProcessDriver, ActivationTransport, CoordinatorActivationJournal, HostError,
    },
};

use super::WorkerState;
use crate::acp::runtime::process_driver::{DriverMailbox, FirstTurnWorkerState, StdinTransport};

pub(super) struct ActivationCompletionFailure<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    pub(super) driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    pub(super) error: HostError,
}

pub(super) async fn complete<A, R, T>(
    mut driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    mailbox: &DriverMailbox,
) -> Result<WorkerState<A, R, T>, Box<ActivationCompletionFailure<A, R, T>>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    if driver.begin().is_err() {
        return Err(Box::new(ActivationCompletionFailure {
            driver,
            error: HostError::Unavailable,
        }));
    }
    loop {
        if driver.completion_ready(mailbox) {
            return match driver.try_into_idle_parts(mailbox) {
                Ok(parts) => Ok(FirstTurnWorkerState::Idle(parts)),
                Err(failure) => Err(Box::new(ActivationCompletionFailure {
                    driver: failure.driver,
                    error: HostError::RecoveryRequired,
                })),
            };
        }
        if driver.run_next(mailbox).await.is_err() {
            return Err(Box::new(ActivationCompletionFailure {
                driver,
                error: HostError::RecoveryRequired,
            }));
        }
    }
}
