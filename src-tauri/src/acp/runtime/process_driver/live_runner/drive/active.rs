use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{
        DriverClock, DriverError, HostError, NonfatalRefusal, PreparedProcessWrite,
        ProjectIntentAuthority, SqliteAdapterBatchJournal, TeardownCause,
    },
    store::AdapterTurnCursor,
};

use super::super::{
    ControlMessage, DriverStep, FirstTurnWorkerContext, FirstTurnWorkerState, RunnerAction,
    RunnerExit, StdinTransport,
    classification::log_unexpected_mailbox,
    configuration::{PendingConfig, fail_pending_config, handle_worker},
    failed, process_tail,
};
use super::wait_deadline;
use crate::acp::runtime::process_driver::live_worker::boxed;

pub(super) async fn step<A, R, T, Auth, C>(
    mut active: Box<PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>>,
    context: &FirstTurnWorkerContext<Auth>,
    pending_config: &mut Option<PendingConfig>,
    eof_deadline_millis: &mut Option<i64>,
) -> Result<(RunnerAction<A, R, T, C>, Option<AdapterTurnCursor>), RunnerExit<A, R, T, C>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    let started = std::time::Instant::now();
    if active.stdout_allowed() {
        return match active
            .turn
            .driver
            .run_next_step_until(
                context.mailbox.as_ref(),
                wait_deadline(
                    pending_config,
                    [
                        *eof_deadline_millis,
                        active.turn.driver.pending_exit_deadline_millis(),
                        active.pending_steer_deadline_millis(context.clock.as_ref()),
                    ],
                    context.clock.as_ref(),
                ),
            )
            .await
        {
            Ok(DriverStep::Progress(_)) => Ok((
                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                None,
            )),
            Ok(DriverStep::YieldWorker { envelope, .. }) => Ok(handle_worker(
                FirstTurnWorkerState::Active(active),
                context,
                envelope,
                pending_config,
            )
            .await),
            Err(DriverError::Refused(NonfatalRefusal::ConfigMutation)) => {
                let Some(config) = pending_config.take() else {
                    return Err(failed(FirstTurnWorkerState::Active(active)));
                };
                config.complete(Err(HostError::ConfigMutationRejected));
                Ok((
                    RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                    None,
                ))
            }
            Err(DriverError::Refused(NonfatalRefusal::GoalMutation)) => {
                let Some(config) = pending_config.take() else {
                    return Err(failed(FirstTurnWorkerState::Active(active)));
                };
                config.complete(Err(HostError::GoalMutationRejected));
                Ok((
                    RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                    None,
                ))
            }
            Err(DriverError::Refused(NonfatalRefusal::SignIn)) => Ok((
                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                None,
            )),
            Err(DriverError::TeardownRequired(TeardownCause::ProcessExit)) => Err(boxed(|| {
                super::super::process_exit::reconcile(*active, context, pending_config)
            })
            .await),
            Err(DriverError::Tree) if active.turn.driver.terminal_outcome().is_some() => {
                Err(boxed(|| {
                    super::super::process_exit::reconcile(*active, context, pending_config)
                })
                .await)
            }
            Err(_) if active.has_durable_terminal() => {
                match context.mailbox.wait_control_until(
                    wait_deadline(
                        pending_config,
                        [
                            *eof_deadline_millis,
                            active.turn.driver.pending_exit_deadline_millis(),
                            active.pending_steer_deadline_millis(context.clock.as_ref()),
                        ],
                        context.clock.as_ref(),
                    ),
                    context.clock.as_ref(),
                ) {
                    Ok(Some(ControlMessage::Worker(envelope))) => Ok(handle_worker(
                        FirstTurnWorkerState::Active(active),
                        context,
                        envelope,
                        pending_config,
                    )
                    .await),
                    Ok(Some(message)) if process_tail(&message) => Ok((
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    )),
                    Ok(None) if pending_config.is_some() || active.steer_pending() => Ok((
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    )),
                    _ => Err(failed(FirstTurnWorkerState::Active(active))),
                }
            }
            Err(error) => {
                fail_pending_config(pending_config, HostError::RecoveryRequired);
                tracing::warn!(
                    activation_checkpoint = "live_runner.drive.active_stdout",
                    ?error,
                    "ACP live runner failed while awaiting stdout"
                );
                Err(failed(FirstTurnWorkerState::Active(active)))
            }
        };
    }
    let deadline = wait_deadline(
        pending_config,
        [
            *eof_deadline_millis,
            active.turn.driver.pending_exit_deadline_millis(),
            active.pending_steer_deadline_millis(context.clock.as_ref()),
        ],
        context.clock.as_ref(),
    );
    let next = if active.awaiting_write_observation() {
        context
            .mailbox
            .wait_write_observation_until(deadline, context.clock.as_ref())
    } else {
        context
            .mailbox
            .wait_next_until(deadline, context.clock.as_ref())
    };
    match next {
        Ok(Some(ControlMessage::Worker(envelope))) => Ok(handle_worker(
            FirstTurnWorkerState::Active(active),
            context,
            envelope,
            pending_config,
        )
        .await),
        Ok(Some(message)) if active.has_durable_terminal() && process_tail(&message) => Ok((
            RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
            None,
        )),
        Ok(None) => {
            match settle_due_exit(
                &mut active,
                context.mailbox.as_ref(),
                context.clock.as_ref(),
            )
            .await
            {
                Some(
                    Err(DriverError::TeardownRequired(TeardownCause::ProcessExit))
                    | Err(DriverError::Tree),
                ) if active.turn.driver.terminal_outcome().is_some() => Err(boxed(|| {
                    super::super::process_exit::reconcile(*active, context, pending_config)
                })
                .await),
                Some(Err(_)) | Some(Ok(())) => Err(failed(FirstTurnWorkerState::Active(active))),
                None if pending_config.is_some()
                    || eof_deadline_millis.is_some()
                    || active.steer_pending() =>
                {
                    Ok((
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    ))
                }
                None => Err(failed(FirstTurnWorkerState::Active(active))),
            }
        }
        Ok(Some(ControlMessage::ReaderTerminal(crate::acp::runtime::ReaderTerminal::Eof))) => {
            match active
                .turn
                .driver
                .observe_reader_eof(context.clock.now_millis())
                .await
            {
                Ok(()) => {
                    *eof_deadline_millis = active.turn.driver.pending_exit_deadline_millis();
                    Ok((
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    ))
                }
                Err(DriverError::TeardownRequired(TeardownCause::ProcessExit))
                | Err(DriverError::Tree)
                    if active.turn.driver.terminal_outcome().is_some() =>
                {
                    Err(boxed(|| {
                        super::super::process_exit::reconcile(*active, context, pending_config)
                    })
                    .await)
                }
                Err(_) => Err(failed(FirstTurnWorkerState::Active(active))),
            }
        }
        Ok(Some(ControlMessage::ProcessExit(exit))) => {
            match active
                .turn
                .driver
                .observe_process_exit(exit, context.clock.now_millis())
                .await
            {
                Ok(()) => Ok((
                    RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                    None,
                )),
                Err(DriverError::TeardownRequired(TeardownCause::ProcessExit))
                | Err(DriverError::Tree)
                    if active.turn.driver.terminal_outcome().is_some() =>
                {
                    Err(boxed(|| {
                        super::super::process_exit::reconcile(*active, context, pending_config)
                    })
                    .await)
                }
                Err(_) => Err(failed(FirstTurnWorkerState::Active(active))),
            }
        }
        result => {
            fail_pending_config(pending_config, HostError::RecoveryRequired);
            log_unexpected_mailbox("live_runner.drive.active_control", &result);
            crate::acp::runtime::prompt_diagnostics::failed(
                "runner.awaiting_write_observation",
                started,
                &HostError::RecoveryRequired,
                (
                    "message",
                    result
                        .as_ref()
                        .ok()
                        .and_then(|message| message.as_ref())
                        .map(super::super::classification::control_kind),
                    "stdout_allowed",
                    active.stdout_allowed(),
                    "ready_for_write",
                    active.ready_for_write(),
                    "terminal",
                    active.turn.driver.terminal_outcome(),
                ),
            );
            Err(failed(FirstTurnWorkerState::Active(active)))
        }
    }
}

async fn settle_due_exit<A, R, T, C>(
    active: &mut PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    mailbox: &super::super::super::DriverMailbox,
    clock: &dyn DriverClock,
) -> Option<Result<(), DriverError>>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
{
    let deadline = active.turn.driver.pending_exit_deadline_millis()?;
    if clock.now_millis() < deadline {
        return None;
    }
    Some(
        active
            .turn
            .driver
            .run_next_step_until(mailbox, Some(deadline))
            .await
            .map(|_| ()),
    )
}
