use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        CoordinatorError, HostError, JournalError, PreparedWriteError, ProjectIntentAuthority,
        SqliteAdapterBatchJournal,
    },
    store::{SendObservation, StoreError},
};

use super::{
    FirstTurnWorkerContext, FirstTurnWorkerState, RunnerExit, StdinTransport,
    configuration::{PendingConfig, fail_pending_config},
    failed,
};
use crate::acp::runtime::PreparedProcessWrite;

pub(super) async fn reconcile<A, R, T, Auth, C>(
    mut active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
    pending_config: &mut Option<PendingConfig>,
) -> RunnerExit<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    // A pending Steer exists only after its complete frame was written. Losing
    // the process before a correlated response cannot prove non-delivery.
    if active.steer_pending() {
        active.reject_steer(HostError::SteerOutcomeUnknown);
    }
    if active.turn.driver.take_durable_mutation_unknown() {
        active = active.mark_durable_mutation_unknown();
    }
    active = match reconcile_unknown(active, context).await {
        Ok(active) => active,
        Err(active) => return fail(active, pending_config),
    };
    if !active.turn.dispatch.is_terminal()
        || (matches!(
            active.turn.dispatch.state(),
            crate::acp::lifecycle::TurnState::DeliveryUnknown
                | crate::acp::lifecycle::TurnState::Interrupted
        ) && active.turn.driver.terminal_outcome().is_some())
    {
        active = match active
            .observe(SendObservation::ProcessLost, context.clock.now_millis())
            .await
        {
            Ok(active) => active,
            Err(failure) if commit_unknown(&failure.error) => {
                let active = failure.state.mark_durable_mutation_unknown();
                match reconcile_unknown(active, context).await {
                    Ok(active) => active,
                    Err(active) => return fail(active, pending_config),
                }
            }
            Err(failure) => return fail(failure.state, pending_config),
        };
    }
    fail(active, pending_config)
}

async fn reconcile_unknown<A, R, T, Auth, C>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
) -> Result<
    PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    if !active.durable_mutation_unknown() {
        return Ok(active);
    }
    active
        .reconcile_durable_mutation_unknown(context.clock.now_millis())
        .await
        .map(|(active, _)| active)
        .map_err(|failure| failure.state)
}

fn fail<A, R, T, C>(
    active: PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    pending_config: &mut Option<PendingConfig>,
) -> RunnerExit<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    fail_pending_config(pending_config, HostError::RecoveryRequired);
    failed(FirstTurnWorkerState::Active(active))
}

fn commit_unknown(error: &PreparedWriteError) -> bool {
    matches!(
        error,
        PreparedWriteError::Coordinator(CoordinatorError::Journal(JournalError::Store(
            StoreError::CommitUnknown
        )))
    )
}
