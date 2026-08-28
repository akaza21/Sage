use std::sync::Weak;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::TreeExit,
    runtime::{ProjectIntentAuthority, RuntimeHost},
    store::AdapterTurnCursor,
};

use super::super::WorkerCommand;
use super::{
    ControlMessage, DriverStep, FirstTurnWorkerContext, FirstTurnWorkerState, RunnerAction,
    RunnerExit, StdinTransport, WorkerEnvelope, WorkerState, acknowledge_terminal, closing, failed,
    process_tail, terminal, terminal_cursor,
};

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
    loop {
        if let Err(error) = terminal::queue(&state, &host, &notified, &mut pending) {
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
                    .run_next_step(context.mailbox.as_ref())
                    .await
                {
                    Ok(DriverStep::Progress(_)) => (
                        RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                        None,
                    ),
                    Ok(DriverStep::YieldWorker { envelope, .. }) => {
                        handle_worker(FirstTurnWorkerState::Active(active), context, envelope).await
                    }
                    Err(_) if active.has_durable_terminal() => {
                        match context
                            .mailbox
                            .wait_control_until(None, context.clock.as_ref())
                        {
                            Ok(Some(ControlMessage::Worker(envelope))) => {
                                handle_worker(
                                    FirstTurnWorkerState::Active(active),
                                    context,
                                    envelope,
                                )
                                .await
                            }
                            Ok(Some(message)) if process_tail(&message) => (
                                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                                None,
                            ),
                            _ => return failed(FirstTurnWorkerState::Active(active)),
                        }
                    }
                    Err(error) => {
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
                match context
                    .mailbox
                    .wait_next_until(None, context.clock.as_ref())
                {
                    Ok(Some(ControlMessage::Worker(envelope))) => {
                        handle_worker(FirstTurnWorkerState::Active(active), context, envelope).await
                    }
                    Ok(Some(message))
                        if active.has_durable_terminal() && process_tail(&message) =>
                    {
                        (
                            RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                            None,
                        )
                    }
                    result => {
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
                match context
                    .mailbox
                    .wait_control_until(None, context.clock.as_ref())
                {
                    Ok(Some(ControlMessage::Worker(envelope))) => {
                        handle_worker(FirstTurnWorkerState::Active(active), context, envelope).await
                    }
                    Ok(Some(message))
                        if active.has_durable_terminal() && process_tail(&message) =>
                    {
                        (
                            RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                            None,
                        )
                    }
                    result => {
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
            FirstTurnWorkerState::Idle(mut idle) => match context
                .mailbox
                .wait_next_until(None, context.clock.as_ref())
            {
                Ok(Some(ControlMessage::StdoutChunk(chunk)))
                    if idle.accept_idle_stdout_chunk(&chunk).is_ok() =>
                {
                    (
                        RunnerAction::Continue(FirstTurnWorkerState::Idle(idle)),
                        None,
                    )
                }
                Ok(Some(ControlMessage::Worker(envelope))) => {
                    handle_worker(FirstTurnWorkerState::Idle(idle), context, envelope).await
                }
                result => {
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
            retained => match context
                .mailbox
                .wait_next_until(None, context.clock.as_ref())
            {
                Ok(Some(ControlMessage::Worker(envelope))) => {
                    handle_worker(retained, context, envelope).await
                }
                Ok(Some(message)) if closing(&retained) && process_tail(&message) => {
                    (RunnerAction::Continue(retained), None)
                }
                result => {
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
                    return failed(next);
                }
                state = next;
            }
            RunnerAction::Failure(state) => return failed(state),
            RunnerAction::Shutdown(state, envelope) => {
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

async fn handle_worker<A, R, T, Auth>(
    state: WorkerState<A, R, T>,
    context: &FirstTurnWorkerContext<Auth>,
    envelope: WorkerEnvelope,
) -> (RunnerAction<A, R, T>, Option<AdapterTurnCursor>)
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
{
    if matches!(&envelope.command, WorkerCommand::Shutdown) {
        let mut state = state;
        state.close_stdin_after_durable_completed();
        return (RunnerAction::Shutdown(Box::new(state), envelope), None);
    }
    let mut terminal_callback = terminal_cursor(&envelope);
    let mut state = state.handle(context, envelope).await;
    loop {
        let FirstTurnWorkerState::Active(mut active) = state else {
            return (RunnerAction::Continue(state), terminal_callback);
        };
        if !active.stdout_allowed() {
            return (
                RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                terminal_callback,
            );
        }
        match active
            .turn
            .driver
            .resume_after_worker_control(context.mailbox.as_ref())
            .await
        {
            Ok(DriverStep::Progress(_)) => {
                return (
                    RunnerAction::Continue(FirstTurnWorkerState::Active(active)),
                    terminal_callback,
                );
            }
            Ok(DriverStep::YieldWorker { envelope, .. }) => {
                if matches!(&envelope.command, WorkerCommand::Shutdown) {
                    let mut state = FirstTurnWorkerState::Active(active);
                    state.close_stdin_after_durable_completed();
                    return (
                        RunnerAction::Shutdown(Box::new(state), envelope),
                        terminal_callback,
                    );
                }
                if let Some(cursor) = terminal_cursor(&envelope)
                    && terminal_callback.replace(cursor).is_some()
                {
                    return (
                        RunnerAction::Failure(FirstTurnWorkerState::Active(active)),
                        terminal_callback,
                    );
                }
                state = FirstTurnWorkerState::Active(active)
                    .handle(context, envelope)
                    .await;
            }
            Err(_) => {
                return (
                    RunnerAction::Failure(FirstTurnWorkerState::Active(active)),
                    terminal_callback,
                );
            }
        }
    }
}
