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
        if terminal::queue(&state, &host, &notified, &mut pending).is_err() {
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
                    Err(_) => return failed(FirstTurnWorkerState::Active(active)),
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
                    _ => return failed(FirstTurnWorkerState::Active(active)),
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
                    _ => return failed(FirstTurnWorkerState::Active(active)),
                }
            }
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
                _ => return failed(retained),
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
