use std::sync::Weak;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        PreparedProcessWrite, ProjectIntentAuthority, RuntimeHost, SessionTreeLease,
        SqliteAdapterBatchJournal,
    },
};

use super::{
    FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState, RunnerExit,
    StdinTransport, WorkerState, failed,
};
use crate::acp::runtime::process_driver::{CompletedDriverParts, live_worker::boxed};

type ReusableParts<A, R, T, C> =
    CompletedDriverParts<A, SqliteAdapterBatchJournal, R, T, C, SessionTreeLease>;

/// Opens the Turn the harness started on its own after the last one ended,
/// so its output is read and shown like any prompted Turn's.
pub(super) async fn open<A, R, T, Auth, C>(
    parts: Box<ReusableParts<A, R, T, C>>,
    context: &FirstTurnWorkerContext<Auth>,
    host: &Weak<RuntimeHost>,
) -> Result<WorkerState<A, R, T, C>, RunnerExit<A, R, T, C>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    let now_millis = context.clock.now_millis();
    let turn = match boxed(|| parts.open_harness_turn(&context.coordinator, now_millis)).await {
        Ok(turn) => turn,
        Err(failure) => {
            tracing::warn!(
                activation_checkpoint = "live_runner.harness_turn.open",
                error = %failure.error,
                "ACP live runner could not open a harness-started Turn"
            );
            return Err(failed(FirstTurnWorkerState::Recovery(Box::new(
                FirstTurnWorkerRecovery::PrepareNext(failure),
            ))));
        }
    };
    let turn_id = turn.dispatch.turn_id().clone();
    let mut active = Box::new(PreparedProcessWrite::started_by_harness(turn));
    if let Err(error) = host
        .upgrade()
        .ok_or(crate::acp::runtime::HostError::Closed)
        .and_then(|host| host.try_notify_turn_started(context.owner.clone(), turn_id))
    {
        tracing::warn!(
            activation_checkpoint = "live_runner.harness_turn.notify",
            ?error,
            "ACP live runner could not report a harness-started Turn"
        );
        return Err(failed(FirstTurnWorkerState::Active(active)));
    }
    if let Err(error) = active.turn.driver.adopt_started_turn(now_millis).await {
        tracing::warn!(
            activation_checkpoint = "live_runner.harness_turn.adopt",
            ?error,
            "ACP live runner could not read a harness-started Turn"
        );
        return Err(failed(FirstTurnWorkerState::Active(active)));
    }
    Ok(FirstTurnWorkerState::Active(active))
}
