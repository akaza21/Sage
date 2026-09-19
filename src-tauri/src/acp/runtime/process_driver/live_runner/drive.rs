use std::sync::Weak;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::{DriverError, HostError, ProjectIntentAuthority, RuntimeHost},
};

use super::{
    ControlMessage, DriverStep, FirstTurnWorkerContext, FirstTurnWorkerState, RunnerAction,
    RunnerExit, StdinTransport, WorkerState, acknowledge_terminal, closing,
    configuration::{PendingConfig, handle_worker},
    failed, process_tail, terminal,
};

fn pending_deadline_millis(
    pending: &Option<PendingConfig>,
    clock: &dyn crate::acp::runtime::DriverClock,
) -> Option<i64> {
    pending.as_ref().map(|config| config.deadline_millis(clock))
}

pub(super) async fn drive<A, R, T, Auth>(
    mut state: WorkerState<A, R, T>,
    context: &FirstTurnWorkerContext<Auth>,
    host: Weak<RuntimeHost>,
) -> RunnerExit<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
{
    let mut notified = None;
    let mut pending = None;
    let mut pending_config: Option<PendingConfig> = None;
    loop {
        if let Some(config) = pending_config.take() {
            if !state.config_pending() {
                config.complete(Ok(()));
            } else if config.expired() {
                config.complete(Err(HostError::Deadline));
            } else {
                pending_config = Some(config);
            }
        }
        if let Err(error) = terminal::queue(&state, &host, &notified, &mut pending) {
            if let Some(config) = pending_config.take() {
                config.complete(Err(HostError::RecoveryRequired));
            }
            tracing::warn!(
                activation_checkpoint = "live_runner.drive.terminal_notification",
                ?error,
                "ACP live runner failed before receiving a worker command"
            );
            return failed(state);
        }
        let (action, terminal_callback) = match state {
            FirstTurnWorkerState::Active(mut active) if active.stdout_allowed() => {
                match active
                    .turn
                    .driver
                    .run_next_step_until(
                        context.mailbox.as_ref(),
                        pending_deadline_millis(&pending_config, context.clock.as_ref()),
                    )
                    .await
                {
                    Ok(DriverStep::Progress(_)) => (
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    ),
                    Ok(DriverStep::YieldWorker { envelope, .. }) => {
                        handle_worker(
                            FirstTurnWorkerState::Active(active),
                            context,
                            envelope,
                            &mut pending_config,
                        )
                        .await
                    }
                    Err(DriverError::ConfigMutationRejected) => {
                        let Some(config) = pending_config.take() else {
                            return failed(FirstTurnWorkerState::Active(active));
                        };
                        config.complete(Err(HostError::ConfigMutationRejected));
                        (
                            RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                            None,
                        )
                    }
                    Err(_) if active.has_durable_terminal() => {
                        match context.mailbox.wait_control_until(
                            pending_deadline_millis(&pending_config, context.clock.as_ref()),
                            context.clock.as_ref(),
                        ) {
                            Ok(Some(ControlMessage::Worker(envelope))) => {
                                handle_worker(
                                    FirstTurnWorkerState::Active(active),
                                    context,
                                    envelope,
                                    &mut pending_config,
                                )
                                .await
                            }
                            Ok(Some(message)) if process_tail(&message) => (
                                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                                None,
                            ),
                            Ok(None) if pending_config.is_some() => (
                                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                                None,
                            ),
                            _ => return failed(FirstTurnWorkerState::Active(active)),
                        }
                    }
                    Err(error) => {
                        if let Some(config) = pending_config.take() {
                            config.complete(Err(HostError::RecoveryRequired));
                        }
                        tracing::warn!(
                            activation_checkpoint = "live_runner.drive.active_stdout",
                            ?error,
                            "ACP live runner failed while awaiting stdout"
                        );
                        return failed(FirstTurnWorkerState::Active(active));
                    }
                }
            }
            FirstTurnWorkerState::Active(active) if active.ready_for_write() => {
                match context.mailbox.wait_next_until(
                    pending_deadline_millis(&pending_config, context.clock.as_ref()),
                    context.clock.as_ref(),
                ) {
                    Ok(Some(ControlMessage::Worker(envelope))) => {
                        handle_worker(
                            FirstTurnWorkerState::Active(active),
                            context,
                            envelope,
                            &mut pending_config,
                        )
                        .await
                    }
                    Ok(Some(message))
                        if active.has_durable_terminal() && process_tail(&message) =>
                    {
                        (
                            RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                            None,
                        )
                    }
                    Ok(None) if pending_config.is_some() => (
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    ),
                    result => {
                        if let Some(config) = pending_config.take() {
                            config.complete(Err(HostError::RecoveryRequired));
                        }
                        tracing::warn!(
                            activation_checkpoint = "live_runner.drive.active_write",
                            outcome = ?result
                                .as_ref()
                                .ok()
                                .and_then(|message| message.as_ref())
                                .map(control_kind),
                            "ACP live runner rejected an unexpected activation mailbox message"
                        );
                        return failed(FirstTurnWorkerState::Active(active));
                    }
                }
            }
            FirstTurnWorkerState::Active(active) => {
                match context.mailbox.wait_control_until(
                    pending_deadline_millis(&pending_config, context.clock.as_ref()),
                    context.clock.as_ref(),
                ) {
                    Ok(Some(ControlMessage::Worker(envelope))) => {
                        handle_worker(
                            FirstTurnWorkerState::Active(active),
                            context,
                            envelope,
                            &mut pending_config,
                        )
                        .await
                    }
                    Ok(Some(message))
                        if active.has_durable_terminal() && process_tail(&message) =>
                    {
                        (
                            RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                            None,
                        )
                    }
                    Ok(None) if pending_config.is_some() => (
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    ),
                    result => {
                        if let Some(config) = pending_config.take() {
                            config.complete(Err(HostError::RecoveryRequired));
                        }
                        tracing::warn!(
                            activation_checkpoint = "live_runner.drive.active_control",
                            outcome = ?result
                                .as_ref()
                                .ok()
                                .and_then(|message| message.as_ref())
                                .map(control_kind),
                            "ACP live runner rejected an unexpected control mailbox message"
                        );
                        return failed(FirstTurnWorkerState::Active(active));
                    }
                }
            }
            FirstTurnWorkerState::Idle(mut idle) => match context.mailbox.wait_next_until(
                pending_deadline_millis(&pending_config, context.clock.as_ref()),
                context.clock.as_ref(),
            ) {
                Ok(Some(ControlMessage::StdoutChunk(chunk))) => {
                    match idle.accept_idle_stdout_chunk(&chunk) {
                        Ok(()) => (
                            RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                            None,
                        ),
                        Err(crate::acp::runtime::AdapterPumpError::ConfigMutationRejected) => {
                            let Some(config) = pending_config.take() else {
                                return failed(FirstTurnWorkerState::Idle(idle));
                            };
                            config.complete(Err(HostError::ConfigMutationRejected));
                            (
                                RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                                None,
                            )
                        }
                        Err(_) => {
                            if let Some(config) = pending_config.take() {
                                config.complete(Err(HostError::RecoveryRequired));
                            }
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
                Ok(None) if pending_config.is_some() => (
                    RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                    None,
                ),
                result => {
                    if let Some(config) = pending_config.take() {
                        config.complete(Err(HostError::RecoveryRequired));
                    }
                    tracing::warn!(
                        activation_checkpoint = "live_runner.drive.idle",
                        outcome = ?result
                            .as_ref()
                            .ok()
                            .and_then(|message| message.as_ref())
                            .map(control_kind),
                        "ACP idle runner rejected an unexpected mailbox message"
                    );
                    return failed(FirstTurnWorkerState::Idle(idle));
                }
            },
            FirstTurnWorkerState::Reusable(mut parts) => match context.mailbox.wait_next_until(
                pending_deadline_millis(&pending_config, context.clock.as_ref()),
                context.clock.as_ref(),
            ) {
                Ok(Some(ControlMessage::StdoutChunk(chunk))) => {
                    match parts.completed.accept_idle_stdout_chunk(&chunk) {
                        Ok(()) => (
                            RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                            None,
                        ),
                        Err(crate::acp::runtime::AdapterPumpError::ConfigMutationRejected) => {
                            let Some(config) = pending_config.take() else {
                                return failed(FirstTurnWorkerState::Reusable(parts));
                            };
                            config.complete(Err(HostError::ConfigMutationRejected));
                            (
                                RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                                None,
                            )
                        }
                        Err(_) => {
                            if let Some(config) = pending_config.take() {
                                config.complete(Err(HostError::RecoveryRequired));
                            }
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
                Ok(None) if pending_config.is_some() => (
                    RunnerAction::Continue(FirstTurnWorkerState::Reusable(parts)),
                    None,
                ),
                _ => {
                    if let Some(config) = pending_config.take() {
                        config.complete(Err(HostError::RecoveryRequired));
                    }
                    return failed(FirstTurnWorkerState::Reusable(parts));
                }
            },
            retained => match context.mailbox.wait_next_until(
                pending_deadline_millis(&pending_config, context.clock.as_ref()),
                context.clock.as_ref(),
            ) {
                Ok(Some(ControlMessage::Worker(envelope))) => {
                    handle_worker(retained, context, envelope, &mut pending_config).await
                }
                Ok(Some(message)) if closing(&retained) && process_tail(&message) => {
                    (RunnerAction::Continue(retained), None)
                }
                Ok(None) if pending_config.is_some() => (RunnerAction::Continue(retained), None),
                result => {
                    if let Some(config) = pending_config.take() {
                        config.complete(Err(HostError::RecoveryRequired));
                    }
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
                    if let Some(config) = pending_config.take() {
                        config.complete(Err(HostError::RecoveryRequired));
                    }
                    return failed(next);
                }
                state = next;
            }
            RunnerAction::Failure(state) => {
                if let Some(config) = pending_config.take() {
                    config.complete(Err(HostError::RecoveryRequired));
                }
                return failed(state);
            }
            RunnerAction::Shutdown(state, envelope) => {
                if let Some(config) = pending_config.take() {
                    config.complete(Err(HostError::Closed));
                }
                let reason = if state.has_durable_completed_cursor()
                    || state.has_durable_cancelled_cursor()
                {
                    TreeExit::Cancelled
                } else {
                    TreeExit::ParentShutdown
                };
                return RunnerExit {
                    state: *state,
                    shutdown: Some(envelope),
                    reason,
                };
            }
        }
    }
}

fn control_kind(message: &ControlMessage) -> &'static str {
    match message {
        ControlMessage::StdoutChunk(_) => "stdout",
        ControlMessage::Prompt(_) => "prompt",
        ControlMessage::Cancel(_) => "cancel",
        ControlMessage::Worker(_) => "worker",
        ControlMessage::ReaderTerminal(crate::acp::runtime::ReaderTerminal::Eof) => "reader-eof",
        ControlMessage::ReaderTerminal(crate::acp::runtime::ReaderTerminal::Io) => "reader-io",
        ControlMessage::ProcessExit(_) => "process-exit",
    }
}
