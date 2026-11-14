use std::{any::Any, sync::Arc, time::Instant};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{TreeControl, TreeExit},
    runtime::{
        ActivationProcessDriver, ActivationTransport, CoordinatorActivationJournal, HostError,
        ProjectIntentAuthority,
    },
};

use super::{
    BlockingStdoutReader, ControlMessage, CoordinatorZeroToken, DriverMailbox, DriverStep,
    FirstTurnWorkerContext, FirstTurnWorkerRecovery, FirstTurnWorkerState, ProcessExitWatcher,
    StdinTransport, WorkerEnvelope,
};
use crate::acp::runtime::BoundedThread;

mod activation;
mod classification;
mod configuration;
mod drive;
mod handle;
mod observability;
mod process_exit;
mod shutdown;
mod terminal;

#[cfg(test)]
pub(super) use drive::active_step_for_test;
use drive::drive;
use handle::RunInput;
pub(crate) use handle::{
    ActivationStartFailure, LiveRunnerExit, LiveRunnerHandle, RunnerStartSubstep,
};
use shutdown::{cleanup_budget, finish};
use terminal::{acknowledge as acknowledge_terminal, cursor as terminal_cursor};

type WorkerState<A, R, T, C = TreeControl> = FirstTurnWorkerState<A, R, T, C>;
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

enum RunnerAction<A, R, T, C = TreeControl>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    Continue(WorkerState<A, R, T, C>),
    Failure(WorkerState<A, R, T, C>),
    Shutdown(Box<WorkerState<A, R, T, C>>, WorkerEnvelope),
}

pub(super) struct RunnerExit<A, R, T, C = TreeControl>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    state: WorkerState<A, R, T, C>,
    shutdown: Option<WorkerEnvelope>,
    pub(super) reason: TreeExit,
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
                RunnerStartSubstep::StdoutReader,
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
                RunnerStartSubstep::ProcessExitWatcher,
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
    let mut initial =
        match sage_core::rt::block_on(activation::complete(activation, mailbox.as_ref())) {
            Ok(initial) => initial,
            Err(failure) => {
                let failure = recover_activation_startup(
                    failure.driver,
                    failure.error,
                    failure.substep,
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
    if let FirstTurnWorkerState::Idle(parts) = &mut initial {
        parts.parts.hydrate_model_config(&mut parts.transport);
        parts.parts.hydrate_goal(&mut parts.transport);
    }
    if ready.send(Ok(())).is_err() {
        let (recovery, _) = finish(
            failed(initial),
            mailbox.as_ref(),
            &control,
            &mut reader,
            &mut watcher,
            cleanup_deadline,
            &context,
        );
        return RunOutcome {
            exit: LiveRunnerExit::Failed,
            recovery,
        };
    }
    let exit = sage_core::rt::block_on(drive(initial, &context, host));
    let (recovery, recovered_after_exit) = finish(
        exit,
        mailbox.as_ref(),
        &control,
        &mut reader,
        &mut watcher,
        cleanup_deadline,
        &context,
    );
    RunOutcome {
        exit: if recovery.is_some() || recovered_after_exit {
            LiveRunnerExit::Failed
        } else {
            LiveRunnerExit::Shutdown
        },
        recovery,
    }
}

fn recover_activation_startup<A, R, T>(
    driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
    error: HostError,
    substep: RunnerStartSubstep,
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
        Ok(handoff) => handle::ActivationStartFailure::Projection {
            handoff,
            error,
            substep,
        },
        Err(failure) => {
            let mut driver = failure.driver;
            driver.fail_before_runner();
            match driver.take_zero_process_handoff_before(deadline) {
                Ok(handoff) => handle::ActivationStartFailure::Teardown {
                    handoff,
                    error,
                    substep,
                },
                Err(failure) => handle::ActivationStartFailure::Retained {
                    driver: failure.driver,
                    error,
                    substep,
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

fn closing<A, R, T, C>(state: &WorkerState<A, R, T, C>) -> bool
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
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
        other => other.has_durable_completed_cursor() || other.has_durable_cancelled_cursor(),
    }
}

fn process_tail(message: &ControlMessage) -> bool {
    matches!(
        message,
        ControlMessage::StdoutChunk(_)
            | ControlMessage::ReaderTerminal(_)
            | ControlMessage::ProcessExit(_)
    )
}

fn failed<A, R, T, C>(state: WorkerState<A, R, T, C>) -> RunnerExit<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    let reason = match &state {
        FirstTurnWorkerState::Active(active) => active
            .turn
            .driver
            .teardown_exit()
            .unwrap_or(TreeExit::ProtocolFailure),
        _ => TreeExit::ProtocolFailure,
    };
    RunnerExit {
        state,
        shutdown: None,
        reason,
    }
}

fn failed_with_exit<A, R, T, C>(
    state: WorkerState<A, R, T, C>,
    reason: TreeExit,
    pending_config: &mut Option<configuration::PendingConfig>,
) -> RunnerExit<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    configuration::fail_pending_config(pending_config, HostError::RecoveryRequired);
    RunnerExit {
        state,
        shutdown: None,
        reason,
    }
}

pub(super) fn requested_shutdown<A, R, T, C>(
    state: WorkerState<A, R, T, C>,
    envelope: WorkerEnvelope,
) -> RunnerExit<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    let reason = if state.has_durable_completed_cursor() || state.has_durable_cancelled_cursor() {
        TreeExit::Cancelled
    } else {
        TreeExit::ParentShutdown
    };
    RunnerExit {
        state,
        shutdown: Some(envelope),
        reason,
    }
}

#[cfg(test)]
pub(super) async fn drive_for_test<A, R, T, Auth, C>(
    state: WorkerState<A, R, T, C>,
    context: &FirstTurnWorkerContext<Auth>,
) -> (WorkerState<A, R, T, C>, TreeExit)
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    Auth: ProjectIntentAuthority,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    let exit = drive(state, context, std::sync::Weak::new()).await;
    (exit.state, exit.reason)
}
