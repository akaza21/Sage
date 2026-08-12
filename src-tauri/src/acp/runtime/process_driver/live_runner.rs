use std::{
    any::Any,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{TreeControl, TreeExit},
    runtime::{
        ActivationProcessDriver, ActivationTransport, CoordinatorActivationJournal, HostError,
        ProjectIntentAuthority,
    },
};

use super::{
    BlockingStdoutReader, ControlMessage, DriverMailbox, DriverStep, FirstTurnWorkerContext,
    FirstTurnWorkerRecovery, FirstTurnWorkerState, ProcessExitWatcher, StdinTransport,
    WorkerEnvelope, WorkerReply,
};
use crate::acp::runtime::BoundedThread;

mod activation;
mod drive;
mod handle;
mod terminal;

use drive::drive;
use handle::RunInput;
pub(crate) use handle::{ActivationStartFailure, LiveRunnerExit, LiveRunnerHandle};
use terminal::{acknowledge as acknowledge_terminal, cursor as terminal_cursor};

type WorkerState<A, R, T> = FirstTurnWorkerState<A, R, T, TreeControl>;
type RetainedRunnerRecovery = Box<dyn Any + Send>;

struct RunOutcome {
    exit: LiveRunnerExit,
    recovery: Option<RetainedRunnerRecovery>,
}

impl RunOutcome {
    fn failed(recovery: impl Any + Send) -> Self {
        Self {
            exit: LiveRunnerExit::Failed,
            recovery: Some(Box::new(recovery)),
        }
    }
}

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

fn run<A, R, T, Auth>(
    activation: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    input: RunInput<A, R, T, Auth>,
) -> RunOutcome
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
            let failure = recover_activation_startup(
                activation,
                HostError::Unavailable,
                mailbox.as_ref(),
                None,
                None,
                cleanup_deadline,
            );
            return match ready.send(Err(failure)) {
                Ok(()) => RunOutcome {
                    exit: LiveRunnerExit::Failed,
                    recovery: None,
                },
                Err(error) => RunOutcome::failed(error.0),
            };
        }
    };
    let watcher_mailbox = Arc::clone(&mailbox);
    let watcher_control = control.clone();
    let mut watcher = match BoundedThread::spawn("sage-acp-process-exit", move || {
        ProcessExitWatcher::new(watcher_control).run(watcher_mailbox.as_ref());
    }) {
        Ok(watcher) => watcher,
        Err(_) => {
            let failure = recover_activation_startup(
                activation,
                HostError::Unavailable,
                mailbox.as_ref(),
                Some(&mut reader),
                None,
                cleanup_deadline,
            );
            return match ready.send(Err(failure)) {
                Ok(()) => RunOutcome {
                    exit: LiveRunnerExit::Failed,
                    recovery: None,
                },
                Err(error) => RunOutcome::failed(error.0),
            };
        }
    };
    let initial =
        match tauri::async_runtime::block_on(activation::complete(activation, mailbox.as_ref())) {
            Ok(initial) => initial,
            Err(failure) => {
                let failure = recover_activation_startup(
                    failure.driver,
                    failure.error,
                    mailbox.as_ref(),
                    Some(&mut reader),
                    Some(&mut watcher),
                    cleanup_deadline,
                );
                return match ready.send(Err(failure)) {
                    Ok(()) => RunOutcome {
                        exit: LiveRunnerExit::Failed,
                        recovery: None,
                    },
                    Err(error) => RunOutcome::failed(error.0),
                };
            }
        };
    if ready.send(Ok(())).is_err() {
        let recovery = finish(
            failed(initial),
            mailbox.as_ref(),
            &control,
            &mut reader,
            &mut watcher,
            cleanup_deadline,
        );
        return RunOutcome {
            exit: LiveRunnerExit::Failed,
            recovery,
        };
    }
    let exit = tauri::async_runtime::block_on(drive(initial, &context, host));
    let disposition = if exit.shutdown.is_some() {
        LiveRunnerExit::Shutdown
    } else {
        LiveRunnerExit::Failed
    };
    let recovery = finish(
        exit,
        mailbox.as_ref(),
        &control,
        &mut reader,
        &mut watcher,
        cleanup_deadline,
    );
    RunOutcome {
        exit: if recovery.is_some() {
            LiveRunnerExit::Failed
        } else {
            disposition
        },
        recovery,
    }
}

fn recover_activation_startup<A, R, T>(
    driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    error: HostError,
    mailbox: &DriverMailbox,
    reader: Option<&mut BoundedThread>,
    watcher: Option<&mut BoundedThread>,
    deadline: Instant,
) -> handle::ActivationStartFailure<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    mailbox.close();
    let failure = match driver.take_projection_recovery_before(deadline) {
        Ok(handoff) => handle::ActivationStartFailure::Projection { handoff, error },
        Err(failure) => {
            let mut driver = failure.driver;
            driver.fail_before_runner();
            match driver.take_zero_process_handoff_before(deadline) {
                Ok(handoff) => handle::ActivationStartFailure::Teardown { handoff, error },
                Err(failure) => handle::ActivationStartFailure::Retained {
                    driver: failure.driver,
                    error,
                },
            }
        }
    };
    if let Some(reader) = reader {
        let _ = reader.wait(deadline.saturating_duration_since(Instant::now()));
    }
    if let Some(watcher) = watcher {
        let _ = watcher.wait(deadline.saturating_duration_since(Instant::now()));
    }
    failure
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
) -> Option<RetainedRunnerRecovery>
where
    A: HarnessAdapter,
    R: ResourceMapper + Send + 'static,
    T: StdinTransport + Send + 'static,
    A: Send + 'static,
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
    let clean_shutdown =
        exit.shutdown.is_some() && already_closed && converged && reader_done && watcher_done;
    if let Some(envelope) = exit.shutdown {
        envelope.respond(if converged && reader_done && watcher_done {
            Ok(WorkerReply::Unit)
        } else {
            Err(HostError::RecoveryRequired)
        });
    }
    (!clean_shutdown).then(|| Box::new(exit.state) as RetainedRunnerRecovery)
}

fn cleanup_budget() -> Duration {
    Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close)
}
