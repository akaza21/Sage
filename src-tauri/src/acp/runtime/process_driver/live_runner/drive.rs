use std::sync::Weak;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::{HostError, NonfatalRefusal, ProjectIntentAuthority, RuntimeHost},
};

use super::{
    ControlMessage, FirstTurnWorkerContext, FirstTurnWorkerState, RunnerAction, RunnerExit,
    StdinTransport, WorkerState, acknowledge_terminal,
    classification::{control_kind, log_unexpected_mailbox},
    closing,
    configuration::{
        PendingConfig, fail_pending_config, handle_worker, pending_deadline_millis,
        settle_pending_config,
    },
    failed, failed_with_exit, process_tail, terminal,
};

mod active;
#[cfg(test)]
pub(in crate::acp::runtime::process_driver) async fn active_step_for_test<A, R, T, Auth, C>(
    state: WorkerState<A, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
    deadline_millis: i64,
) -> Result<WorkerState<A, R, T, C>, TreeExit>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    let FirstTurnWorkerState::Active(active) = state else {
        panic!("active runner step");
    };
    let mut pending = None;
    let mut deadline = Some(deadline_millis);
    match active::step(active, context, &mut pending, &mut deadline).await {
        Ok((RunnerAction::Continue(next), None)) => Ok(next),
        Ok(_) => panic!("expected a nonterminal runner step"),
        Err(exit) => Err(exit.reason),
    }
}

pub(super) async fn drive<A, R, T, Auth, C>(
    mut state: WorkerState<A, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
    host: Weak<RuntimeHost>,
) -> RunnerExit<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    let mut notified = None;
    let mut pending = None;
    let mut pending_config: Option<PendingConfig> = None;
    let mut eof_deadline_millis = None;
    loop {
        match &mut state {
            FirstTurnWorkerState::Reusable(parts) => {
                parts.completed.hydrate_model_config(&mut parts.transport);
            }
            FirstTurnWorkerState::Idle(parts) => {
                parts.parts.write_discovered_commands(&mut parts.transport);
            }
            _ => {}
        }
        if let FirstTurnWorkerState::Active(active) = &mut state {
            active.settle_steer();
        }
        let native_reply = if pending_config
            .as_ref()
            .is_some_and(PendingConfig::is_native_rpc)
        {
            state.take_native_rpc_reply()
        } else {
            None
        };
        if settle_pending_config(
            &mut pending_config,
            state.config_pending(),
            state.native_rpc_pending(),
            native_reply,
        ) {
            state.abandon_passive_read();
        }
        if let Err(error) = terminal::queue(&state, &host, &notified, &mut pending) {
            fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
            tracing::warn!(
                activation_checkpoint = "live_runner.drive.terminal_notification",
                ?error,
                "ACP live runner failed before receiving a worker command"
            );
            return failed(state);
        }
        let (action, terminal_callback) = match state {
            FirstTurnWorkerState::Active(active) => match active::step(
                active,
                context,
                &mut pending_config,
                &mut eof_deadline_millis,
            )
            .await
            {
                Ok(step) => step,
                Err(exit) => return exit,
            },
            FirstTurnWorkerState::Idle(mut idle) => match context.mailbox.wait_next_until(
                wait_deadline(
                    &pending_config,
                    [eof_deadline_millis],
                    context.clock.as_ref(),
                ),
                context.clock.as_ref(),
            ) {
                Ok(Some(ControlMessage::StdoutChunk(chunk))) => {
                    match idle.accept_idle_stdout_chunk(&chunk) {
                        Ok(()) => (
                            RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                            None,
                        ),
                        Err(crate::acp::runtime::AdapterPumpError::Refused(
                            NonfatalRefusal::ConfigMutation,
                        )) => {
                            let Some(config) = pending_config.take() else {
                                return failed(FirstTurnWorkerState::Idle(idle));
                            };
                            config.complete(Err(HostError::ConfigMutationRejected));
                            (
                                RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                                None,
                            )
                        }
                        Err(crate::acp::runtime::AdapterPumpError::Refused(
                            NonfatalRefusal::GoalMutation,
                        )) => {
                            let Some(config) = pending_config.take() else {
                                return failed(FirstTurnWorkerState::Idle(idle));
                            };
                            config.complete(Err(HostError::GoalMutationRejected));
                            (
                                RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                                None,
                            )
                        }
                        Err(crate::acp::runtime::AdapterPumpError::Refused(
                            NonfatalRefusal::SignIn,
                        )) => (
                            RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                            None,
                        ),
                        Err(_) => {
                            fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                            return failed(FirstTurnWorkerState::Idle(idle));
                        }
                    }
                }
                Ok(Some(ControlMessage::Worker(envelope))) => {
                    handle_worker(
                        FirstTurnWorkerState::Idle(idle),
                        context,
                        envelope,
                        &mut pending_config,
                    )
                    .await
                }
                Ok(Some(ControlMessage::ProcessExit(exit))) => {
                    return failed_with_exit(
                        FirstTurnWorkerState::Idle(idle),
                        exit,
                        &mut pending_config,
                    );
                }
                Ok(Some(ControlMessage::ReaderTerminal(
                    crate::acp::runtime::ReaderTerminal::Eof,
                ))) => {
                    eof_deadline_millis = Some(reader_eof_deadline(context.clock.as_ref()));
                    fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                    (
                        RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                        None,
                    )
                }
                Ok(None) if pending_config.is_some() => (
                    RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                    None,
                ),
                Ok(None) if eof_deadline_millis.is_some() => {
                    return failed_with_exit(
                        FirstTurnWorkerState::Idle(idle),
                        TreeExit::Exited { code: None },
                        &mut pending_config,
                    );
                }
                result => {
                    fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                    log_unexpected_mailbox("live_runner.drive.idle", &result);
                    return failed(FirstTurnWorkerState::Idle(idle));
                }
            },
            FirstTurnWorkerState::Reusable(mut parts) => match context.mailbox.wait_next_until(
                wait_deadline(
                    &pending_config,
                    [eof_deadline_millis],
                    context.clock.as_ref(),
                ),
                context.clock.as_ref(),
            ) {
                Ok(Some(ControlMessage::StdoutChunk(chunk))) => {
                    match parts
                        .completed
                        .accept_idle_stdout_chunk(
                            &chunk,
                            context.clock.now_millis(),
                            &mut parts.transport,
                        )
                        .await
                    {
                        Ok(()) => (
                            RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                            None,
                        ),
                        Err(crate::acp::runtime::AdapterPumpError::Refused(
                            NonfatalRefusal::ConfigMutation,
                        )) => {
                            let Some(config) = pending_config.take() else {
                                return failed(FirstTurnWorkerState::Reusable(parts));
                            };
                            config.complete(Err(HostError::ConfigMutationRejected));
                            (
                                RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                                None,
                            )
                        }
                        Err(crate::acp::runtime::AdapterPumpError::Refused(
                            NonfatalRefusal::GoalMutation,
                        )) => {
                            let Some(config) = pending_config.take() else {
                                return failed(FirstTurnWorkerState::Reusable(parts));
                            };
                            config.complete(Err(HostError::GoalMutationRejected));
                            (
                                RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                                None,
                            )
                        }
                        Err(crate::acp::runtime::AdapterPumpError::Refused(
                            NonfatalRefusal::SignIn,
                        )) => (
                            RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                            None,
                        ),
                        Err(_) => {
                            fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                            return failed(FirstTurnWorkerState::Reusable(parts));
                        }
                    }
                }
                Ok(Some(ControlMessage::Worker(envelope))) => {
                    handle_worker(
                        FirstTurnWorkerState::Reusable(parts),
                        context,
                        envelope,
                        &mut pending_config,
                    )
                    .await
                }
                Ok(Some(ControlMessage::ProcessExit(exit))) => {
                    return failed_with_exit(
                        FirstTurnWorkerState::Reusable(parts),
                        exit,
                        &mut pending_config,
                    );
                }
                Ok(Some(ControlMessage::ReaderTerminal(
                    crate::acp::runtime::ReaderTerminal::Eof,
                ))) => {
                    eof_deadline_millis = Some(reader_eof_deadline(context.clock.as_ref()));
                    fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                    (
                        RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                        None,
                    )
                }
                Ok(None) if pending_config.is_some() => (
                    RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                    None,
                ),
                Ok(None) if eof_deadline_millis.is_some() => {
                    return failed_with_exit(
                        FirstTurnWorkerState::Reusable(parts),
                        TreeExit::Exited { code: None },
                        &mut pending_config,
                    );
                }
                _ => {
                    fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                    return failed(FirstTurnWorkerState::Reusable(parts));
                }
            },
            retained => match context.mailbox.wait_next_until(
                pending_deadline_millis(&pending_config, context.clock.as_ref()),
                context.clock.as_ref(),
            ) {
                Ok(Some(ControlMessage::StdoutChunk(chunk)))
                    if matches!(&retained, FirstTurnWorkerState::Prepared { .. }) =>
                {
                    let mut retained = retained;
                    let FirstTurnWorkerState::Prepared { turn, .. } = &mut retained else {
                        unreachable!()
                    };
                    match turn.parts.accept_idle_stdout_chunk(&chunk) {
                        Ok(()) => (RunnerAction::Continue(retained), None),
                        Err(_) => {
                            fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                            return failed(retained);
                        }
                    }
                }
                Ok(Some(ControlMessage::Worker(envelope))) => {
                    handle_worker(retained, context, envelope, &mut pending_config).await
                }
                Ok(Some(message)) if closing(&retained) && process_tail(&message) => {
                    (RunnerAction::Continue(retained), None)
                }
                Ok(None) if pending_config.is_some() => (RunnerAction::Continue(retained), None),
                result => {
                    fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                    tracing::warn!(
                        activation_checkpoint = "live_runner.drive.retained",
                        outcome = ?result
                            .as_ref()
                            .ok()
                            .and_then(|message| message.as_ref())
                            .map(control_kind),
                        "ACP retained runner rejected an unexpected mailbox message"
                    );
                    return failed(retained);
                }
            },
        };
        match action {
            RunnerAction::Continue(next) => {
                if terminal_callback.as_ref().is_some_and(|cursor| {
                    acknowledge_terminal(cursor, &mut pending, &mut notified).is_err()
                }) {
                    fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                    return failed(next);
                }
                state = next;
            }
            RunnerAction::Failure(state) => {
                fail_pending_config(&mut pending_config, HostError::RecoveryRequired);
                return failed(state);
            }
            RunnerAction::Shutdown(state, envelope) => {
                fail_pending_config(&mut pending_config, HostError::Closed);
                return super::requested_shutdown(*state, envelope);
            }
        }
    }
}

fn reader_eof_deadline(clock: &dyn crate::acp::runtime::DriverClock) -> i64 {
    i64::try_from(sage_core::limits::LIMITS.deadlines_ms.tree_termination)
        .map_or(i64::MAX, |budget| clock.now_millis().saturating_add(budget))
}

pub(super) fn wait_deadline(
    pending_config: &Option<PendingConfig>,
    deadlines: impl IntoIterator<Item = Option<i64>>,
    clock: &dyn crate::acp::runtime::DriverClock,
) -> Option<i64> {
    std::iter::once(pending_deadline_millis(pending_config, clock))
        .chain(deadlines)
        .flatten()
        .min()
}
