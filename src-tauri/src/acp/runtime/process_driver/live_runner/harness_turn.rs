use std::sync::Weak;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        AdapterPumpError, CoordinatorError, HostError, JournalError, PreparedProcessWrite,
        ProjectIntentAuthority, RuntimeHost, SessionTreeLease, SqliteAdapterBatchJournal,
        StartNotification,
    },
    store::StoreError,
};
use sage_acp_protocol::scheduler::SchedulerError;

use super::{
    FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState, RunnerExit,
    StdinTransport, WorkerState,
    configuration::{PendingConfig, settle_idle_refusal},
    failed,
};
use crate::acp::runtime::process_driver::{
    CompletedDriverParts, PrepareDriverTurnError, PrepareDriverTurnFailure, live_worker::boxed,
};

type ReusableParts<A, R, T, C> =
    CompletedDriverParts<A, SqliteAdapterBatchJournal, R, T, C, SessionTreeLease>;

/// Opens or declines each Turn the harness started on its own after the last
/// one ended, until one opens or none is left. A decline reads on through the
/// stdout already buffered after it, which may start the next Turn, so every
/// start that output holds is settled before the runner waits again. An
/// opened Turn's host answer is left in `started` to read.
pub(super) async fn open_started<A, R, T, Auth, C>(
    mut state: WorkerState<A, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
    host: &Weak<RuntimeHost>,
    pending_config: &mut Option<PendingConfig>,
    started: &mut Option<StartNotification>,
) -> Result<WorkerState<A, R, T, C>, RunnerExit<A, R, T, C>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    loop {
        state = match state {
            FirstTurnWorkerState::Reusable(parts) if parts.turn_started() => {
                let (next, notification) = open(parts, context, host, pending_config).await?;
                if notification.is_some() {
                    *started = notification;
                }
                next
            }
            FirstTurnWorkerState::Idle(parts) if parts.parts.turn_started() => {
                let (next, notification) = open_first(parts, context, host).await?;
                if notification.is_some() {
                    *started = notification;
                }
                next
            }
            state => return Ok(state),
        };
    }
}

/// Opens the Turn the harness started on its own after the last one ended,
/// so its output is read and shown like any prompted Turn's. Returns the
/// host's answer still to read, or none when the Turn was declined.
async fn open<A, R, T, Auth, C>(
    parts: Box<ReusableParts<A, R, T, C>>,
    context: &FirstTurnWorkerContext<Auth>,
    host: &Weak<RuntimeHost>,
    pending_config: &mut Option<PendingConfig>,
) -> Result<(WorkerState<A, R, T, C>, Option<StartNotification>), RunnerExit<A, R, T, C>>
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
        Err(failure) if refused(&failure) => {
            tracing::info!(
                activation_checkpoint = "live_runner.harness_turn.declined",
                error = %failure.error,
                "ACP live runner declined a harness-started Turn"
            );
            let mut parts = Box::new(failure.parts);
            match parts
                .completed
                .decline_started_turn(now_millis, &mut parts.transport)
                .await
            {
                Ok(()) => {}
                Err(AdapterPumpError::Refused(refusal))
                    if settle_idle_refusal(pending_config, refusal) => {}
                Err(error) => {
                    tracing::warn!(
                        activation_checkpoint = "live_runner.harness_turn.decline",
                        ?error,
                        "ACP live runner could not decline a harness-started Turn"
                    );
                    return Err(failed(FirstTurnWorkerState::Reusable(parts)));
                }
            }
            return Ok((FirstTurnWorkerState::Reusable(parts), None));
        }
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
    activate(turn, context, host).await
}

async fn open_first<A, R, T, Auth, C>(
    parts: Box<
        crate::acp::runtime::process_driver::CompletedActivationDriverParts<
            A,
            crate::acp::runtime::CoordinatorActivationJournal,
            R,
            T,
            C,
        >,
    >,
    context: &FirstTurnWorkerContext<Auth>,
    host: &Weak<RuntimeHost>,
) -> Result<(WorkerState<A, R, T, C>, Option<StartNotification>), RunnerExit<A, R, T, C>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    let prepared = match parts
        .prepare_first_harness_turn(&context.coordinator, context.clock.now_millis())
        .await
    {
        Ok(turn) => turn,
        Err(failure) => {
            if failure.recovery.is_none() {
                let mut parts = Box::new(failure.parts);
                if parts
                    .parts
                    .decline_started_turn(&mut parts.transport)
                    .is_err()
                    || parts.accept_idle_stdout_chunk(&[]).is_err()
                {
                    return Err(failed(FirstTurnWorkerState::Idle(parts)));
                }
                return Ok((FirstTurnWorkerState::Idle(parts), None));
            }
            return Err(failed(FirstTurnWorkerState::Recovery(Box::new(
                FirstTurnWorkerRecovery::Prepare(failure),
            ))));
        }
    };
    let turn = match prepared.bind_harness_driver(
        context.journal.clone(),
        std::sync::Arc::clone(&context.publisher),
    ) {
        Ok(turn) => turn,
        Err(failure) => {
            return Err(failed(FirstTurnWorkerState::Recovery(Box::new(
                FirstTurnWorkerRecovery::Bind(failure),
            ))));
        }
    };
    activate(turn, context, host).await
}

async fn activate<A, R, T, Auth, C>(
    turn: crate::acp::runtime::process_driver::PreparedDriverTurn<
        A,
        SqliteAdapterBatchJournal,
        R,
        T,
        C,
    >,
    context: &FirstTurnWorkerContext<Auth>,
    host: &Weak<RuntimeHost>,
) -> Result<(WorkerState<A, R, T, C>, Option<StartNotification>), RunnerExit<A, R, T, C>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    let turn_id = turn.dispatch.turn_id().clone();
    let mut active = Box::new(PreparedProcessWrite::started_by_harness(turn));
    let notification = match host
        .upgrade()
        .ok_or(HostError::Closed)
        .and_then(|host| host.try_notify_turn_started(context.owner.clone(), turn_id))
    {
        Ok(notification) => notification,
        Err(error) => {
            tracing::warn!(
                activation_checkpoint = "live_runner.harness_turn.notify",
                ?error,
                "ACP live runner could not report a harness-started Turn"
            );
            return Err(failed(FirstTurnWorkerState::Active(active)));
        }
    };
    if let Err(error) = active
        .turn
        .driver
        .adopt_started_turn(context.clock.now_millis())
        .await
    {
        tracing::warn!(
            activation_checkpoint = "live_runner.harness_turn.adopt",
            ?error,
            "ACP live runner could not read a harness-started Turn"
        );
        return Err(failed(FirstTurnWorkerState::Active(active)));
    }
    Ok((FirstTurnWorkerState::Active(active), Some(notification)))
}

/// The Turn was refused before anything opened: it would run in another
/// mode than the chat's access allows now, the runtime holds a Turn the
/// user undid or no longer runs, the session's history or Turn slot is
/// full, or Sage is switching branches in the Project's repository. The run
/// is declined and the session stays.
fn refused<P>(failure: &PrepareDriverTurnFailure<P>) -> bool {
    failure.recovery.is_none()
        && matches!(
            &failure.error,
            PrepareDriverTurnError::Prepare(
                CoordinatorError::Intent
                    | CoordinatorError::Scheduler(SchedulerError::TurnCapacity)
                    | CoordinatorError::Journal(JournalError::Store(
                        StoreError::Capacity { .. }
                            | StoreError::RepositoryBusy(_)
                            | StoreError::Conflict {
                                resource: "sessionEpoch"
                            }
                    ))
            )
        )
}

/// Checks the host's answer to a started Turn on each pass of the runner;
/// a refusal fails the runner at once.
pub(super) fn settle(pending: &mut Option<StartNotification>) -> Result<(), HostError> {
    let Some(notification) = pending else {
        return Ok(());
    };
    match notification.settled() {
        Ok(true) => *pending = None,
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                activation_checkpoint = "live_runner.harness_turn.started",
                ?error,
                "ACP live runner's host refused a harness-started Turn"
            );
            return Err(error);
        }
    }
    Ok(())
}
