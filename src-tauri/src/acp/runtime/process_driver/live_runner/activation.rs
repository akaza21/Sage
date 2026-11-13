use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeControl,
    runtime::{
        ActivationProcessDriver, ActivationTransport, CoordinatorActivationJournal, HostError,
    },
};

use super::WorkerState;
use crate::acp::runtime::process_driver::{
    DriverMailbox, FirstTurnWorkerState, RunnerStartSubstep, StdinTransport,
};

pub(super) struct ActivationCompletionFailure<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    pub(super) driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    pub(super) error: HostError,
    pub(super) substep: RunnerStartSubstep,
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
    if let Err(error) = driver.begin() {
        driver.log_startup_failure("live_runner.activation.begin", &error);
        return Err(Box::new(ActivationCompletionFailure {
            substep: driver.startup_step().into(),
            driver,
            error: HostError::Unavailable,
        }));
    }
    loop {
        if driver.completion_ready(mailbox) {
            return match driver.try_into_idle_parts(mailbox) {
                Ok(parts) => Ok(FirstTurnWorkerState::Idle(Box::new(parts))),
                Err(failure) => {
                    driver = failure.driver;
                    continue;
                }
            };
        }
        if let Err(error) = driver.run_next(mailbox).await {
            driver.log_startup_failure("live_runner.activation.run_next", &error);
            return Err(Box::new(ActivationCompletionFailure {
                substep: driver.startup_step().into(),
                driver,
                error: HostError::RecoveryRequired,
            }));
        }
    }
}
