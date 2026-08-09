use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{TreeControl, TreeExit},
    runtime::{
        ActivationProcessDriver, ActivationTransport, CoordinatorActivationJournal, HostError,
        ProjectIntentAuthority, RuntimeHost,
    },
    store::AdapterTurnCursor,
};

use super::{
    BlockingStdoutReader, ControlMessage, DriverMailbox, DriverStep, FirstTurnWorkerContext,
    FirstTurnWorkerRecovery, FirstTurnWorkerState, ProcessExitWatcher, StdinTransport,
    WorkerCommand, WorkerEnvelope, WorkerReply,
};
use crate::acp::runtime::BoundedThread;

mod activation;
mod handle;
mod terminal;

use handle::RunInput;
pub(crate) use handle::{LiveRunnerExit, LiveRunnerHandle};
use terminal::{acknowledge as acknowledge_terminal, cursor as terminal_cursor};

type WorkerState<A, R, T> = FirstTurnWorkerState<A, R, T, TreeControl>;

enum RunnerAction<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
{
    Continue(WorkerState<A, R, T>),
    Failure(WorkerState<A, R, T>),
    Shutdown(Box<WorkerState<A, R, T>>, WorkerEnvelope),
}

struct RunnerExit<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
{
    state: WorkerState<A, R, T>,
    shutdown: Option<WorkerEnvelope>,
    reason: TreeExit,
}

async fn drive<A, R, T, Auth>(
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
                    Err(_) => return failed(FirstTurnWorkerState::Active(active)),
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
                return RunnerExit {
                    state: *state,
                    shutdown: Some(envelope),
                    reason: TreeExit::ParentShutdown,
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
                    return (
                        RunnerAction::Shutdown(
                            Box::new(FirstTurnWorkerState::Active(active)),
                            envelope,
                        ),
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

fn run<A, R, T, Auth>(
    activation: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    input: RunInput<Auth>,
) -> LiveRunnerExit
where
    A: HarnessAdapter + Send + 'static,
    R: ResourceMapper + Send + 'static,
    T: StdinTransport + ActivationTransport + Send + 'static,
    Auth: ProjectIntentAuthority + 'static,
{
    let RunInput {
        context,
        stdout,
        host,
        mailbox,
        control,
        ready,
        cleanup_deadline,
    } = input;
    let reader_mailbox = Arc::clone(&mailbox);
    let mut reader = match BoundedThread::spawn("sage-acp-stdout", move || {
        BlockingStdoutReader::new(stdout).run(reader_mailbox.as_ref());
    }) {
        Ok(reader) => reader,
        Err(_) => {
            fail_startup(mailbox.as_ref(), &control, None, None, cleanup_deadline);
            let _ = ready.send(Err(HostError::Unavailable));
            return LiveRunnerExit::Failed;
        }
    };
    let watcher_mailbox = Arc::clone(&mailbox);
    let watcher_control = control.clone();
    let mut watcher = match BoundedThread::spawn("sage-acp-process-exit", move || {
        ProcessExitWatcher::new(watcher_control).run(watcher_mailbox.as_ref());
    }) {
        Ok(watcher) => watcher,
        Err(_) => {
            fail_startup(
                mailbox.as_ref(),
                &control,
                Some(&mut reader),
                None,
                cleanup_deadline,
            );
            let _ = ready.send(Err(HostError::Unavailable));
            return LiveRunnerExit::Failed;
        }
    };
    let initial =
        match tauri::async_runtime::block_on(activation::complete(activation, mailbox.as_ref())) {
            Ok(initial) => initial,
            Err(error) => {
                fail_startup(
                    mailbox.as_ref(),
                    &control,
                    Some(&mut reader),
                    Some(&mut watcher),
                    cleanup_deadline,
                );
                let _ = ready.send(Err(error));
                return LiveRunnerExit::Failed;
            }
        };
    if ready.send(Ok(())).is_err() {
        fail_startup(
            mailbox.as_ref(),
            &control,
            Some(&mut reader),
            Some(&mut watcher),
            cleanup_deadline,
        );
        return LiveRunnerExit::Failed;
    }
    let exit = tauri::async_runtime::block_on(drive(initial, &context, host));
    let disposition = if exit.shutdown.is_some() {
        LiveRunnerExit::Shutdown
    } else {
        LiveRunnerExit::Failed
    };
    finish(
        exit,
        mailbox.as_ref(),
        &control,
        &mut reader,
        &mut watcher,
        cleanup_deadline,
    );
    disposition
}

fn fail_startup(
    mailbox: &DriverMailbox,
    control: &TreeControl,
    reader: Option<&mut BoundedThread>,
    watcher: Option<&mut BoundedThread>,
    deadline: Instant,
) {
    let _ = control.request_termination(TreeExit::ParentShutdown);
    mailbox.close();
    let _ = control.wait_before(deadline);
    if let Some(reader) = reader {
        let _ = reader.wait(deadline.saturating_duration_since(Instant::now()));
    }
    if let Some(watcher) = watcher {
        let _ = watcher.wait(deadline.saturating_duration_since(Instant::now()));
    }
}

fn closing<A, R, T>(state: &WorkerState<A, R, T>) -> bool
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
{
    match state {
        FirstTurnWorkerState::ReusableDraining(_)
        | FirstTurnWorkerState::ReusableZero(_)
        | FirstTurnWorkerState::Draining(_)
        | FirstTurnWorkerState::Zero(_)
        | FirstTurnWorkerState::IdleDraining(_)
        | FirstTurnWorkerState::IdleZero(_)
        | FirstTurnWorkerState::Closed => true,
        FirstTurnWorkerState::Recovery(recovery) => matches!(
            recovery.as_ref(),
            FirstTurnWorkerRecovery::BeginClose(_)
                | FirstTurnWorkerRecovery::Converge(_)
                | FirstTurnWorkerRecovery::Finalize(_)
                | FirstTurnWorkerRecovery::IdleBegin(_)
                | FirstTurnWorkerRecovery::IdleConverge(_)
                | FirstTurnWorkerRecovery::IdleFinalize(_)
                | FirstTurnWorkerRecovery::ReusableBegin(_)
                | FirstTurnWorkerRecovery::ReusableConverge(_)
                | FirstTurnWorkerRecovery::ReusableFinalize(_)
        ),
        _ => false,
    }
}

fn process_tail(message: &ControlMessage) -> bool {
    matches!(
        message,
        ControlMessage::StdoutChunk(_)
            | ControlMessage::ReaderTerminal(_)
            | ControlMessage::ProcessExit
    )
}

fn failed<A, R, T>(state: WorkerState<A, R, T>) -> RunnerExit<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
{
    RunnerExit {
        state,
        shutdown: None,
        reason: TreeExit::ProtocolFailure,
    }
}

fn finish<A, R, T>(
    exit: RunnerExit<A, R, T>,
    mailbox: &DriverMailbox,
    control: &TreeControl,
    reader: &mut BoundedThread,
    watcher: &mut BoundedThread,
    cleanup_deadline: Instant,
) where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
{
    let deadline = exit
        .shutdown
        .as_ref()
        .map(WorkerEnvelope::deadline)
        .map(|deadline| deadline.min(cleanup_deadline))
        .unwrap_or(cleanup_deadline);
    let already_closed = matches!(exit.state, FirstTurnWorkerState::Closed);
    if !already_closed {
        let _ = control.request_termination(exit.reason);
    }
    mailbox.close();
    let converged = control.wait_before(deadline).is_ok();
    let reader_done = reader
        .wait(deadline.saturating_duration_since(Instant::now()))
        .is_ok();
    let watcher_done = watcher
        .wait(deadline.saturating_duration_since(Instant::now()))
        .is_ok();
    drop(exit.state);
    if let Some(envelope) = exit.shutdown {
        envelope.respond(if converged && reader_done && watcher_done {
            Ok(WorkerReply::Unit)
        } else {
            Err(HostError::RecoveryRequired)
        });
    }
}

fn cleanup_budget() -> Duration {
    Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close)
}
