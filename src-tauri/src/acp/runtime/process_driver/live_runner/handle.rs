use std::{
    sync::{Arc, Mutex, Weak, mpsc},
    time::Instant,
};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{StdoutEndpoint, TreeControl, TreeExit},
    runtime::{
        ActivationProcessDriver, ActivationProjectionRecoveryHandoff, ActivationTransport,
        ActivationZeroProcessHandoff, BoundedThread, CoordinatorActivationJournal, HostError,
        ProjectIntentAuthority, RuntimeHost, ThreadWaitError,
    },
};

use super::{RetainedRunnerRecovery, RunOutcome, cleanup_budget, run};
use crate::acp::runtime::process_driver::{
    DriverMailbox, FirstTurnWorkerContext, StdinTransport, worker_command::WorkerDeadlineAbort,
};

pub(crate) struct LiveRunnerHandle {
    mailbox: Arc<DriverMailbox>,
    control: TreeControl,
    thread: Mutex<Option<BoundedThread>>,
    exit: Mutex<ExitState>,
    recovery: Mutex<Option<RetainedRunnerRecovery>>,
}

// Recovery variants deliberately retain their unique driver/handoff by value;
// callers must consume exactly one authority instead of sharing indirection.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ActivationStartFailure<A, R, T>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
{
    Teardown {
        handoff: ActivationZeroProcessHandoff<
            A,
            CoordinatorActivationJournal,
            R,
            crate::acp::process::ZeroProcessToken,
        >,
        error: HostError,
    },
    Projection {
        handoff: ActivationProjectionRecoveryHandoff<
            A,
            CoordinatorActivationJournal,
            R,
            crate::acp::process::ZeroProcessToken,
        >,
        error: HostError,
    },
    Retained {
        driver: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
        error: HostError,
    },
    OwnedByRunner(HostError),
}

pub(super) struct RunInput<A, R, T, Auth>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport + ActivationTransport,
    Auth: ProjectIntentAuthority,
{
    pub(super) context: FirstTurnWorkerContext<Auth>,
    pub(super) stdout: StdoutEndpoint,
    pub(super) host: Weak<RuntimeHost>,
    pub(super) mailbox: Arc<DriverMailbox>,
    pub(super) control: TreeControl,
    pub(super) ready: mpsc::SyncSender<Result<(), ActivationStartFailure<A, R, T>>>,
    pub(super) cleanup_deadline: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveRunnerExit {
    Shutdown,
    Failed,
}

type ExitObserver = Box<dyn FnOnce(LiveRunnerExit) + Send + 'static>;

enum ExitState {
    Pending,
    Watching(ExitObserver),
    Exited(LiveRunnerExit),
    Delivered,
}

impl LiveRunnerHandle {
    pub(crate) fn new(mailbox: Arc<DriverMailbox>, control: TreeControl) -> Arc<Self> {
        Arc::new(Self {
            mailbox,
            control,
            thread: Mutex::new(None),
            exit: Mutex::new(ExitState::Pending),
            recovery: Mutex::new(None),
        })
    }

    pub(crate) fn mailbox(&self) -> Arc<DriverMailbox> {
        Arc::clone(&self.mailbox)
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn start_activation<A, R, T, Auth>(
        self: &Arc<Self>,
        activation: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
        context: FirstTurnWorkerContext<Auth>,
        stdout: StdoutEndpoint,
        host: Weak<RuntimeHost>,
        startup_deadline: Instant,
        cleanup_deadline: Instant,
    ) -> Result<(), ActivationStartFailure<A, R, T>>
    where
        A: HarnessAdapter + Send + 'static,
        R: ResourceMapper + Send + 'static,
        T: StdinTransport + ActivationTransport + Send + 'static,
        Auth: ProjectIntentAuthority + 'static,
    {
        if startup_deadline > cleanup_deadline || Instant::now() >= startup_deadline {
            let mut activation = activation;
            activation.fail_before_runner();
            return Err(
                match activation.take_zero_process_handoff_before(cleanup_deadline) {
                    Ok(handoff) => ActivationStartFailure::Teardown {
                        handoff,
                        error: HostError::Deadline,
                    },
                    Err(failure) => ActivationStartFailure::Retained {
                        driver: failure.driver,
                        error: HostError::Deadline,
                    },
                },
            );
        }
        let mut retained = self.lock_thread();
        if retained.is_some() {
            return Err(ActivationStartFailure::Retained {
                driver: activation,
                error: HostError::State,
            });
        }
        let (ready, startup) = mpsc::sync_channel(1);
        let mailbox = Arc::clone(&self.mailbox);
        let control = self.control.clone();
        let runner = Arc::downgrade(self);
        let activation = Arc::new(Mutex::new(Some(activation)));
        let worker_activation = Arc::clone(&activation);
        let worker = BoundedThread::spawn("sage-acp-live-runner", move || {
            let activation = worker_activation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .expect("live activation is taken exactly once");
            let outcome = run(
                activation,
                RunInput {
                    context,
                    stdout,
                    host,
                    mailbox,
                    control,
                    ready,
                    cleanup_deadline,
                },
            );
            if let Some(runner) = runner.upgrade() {
                runner.record_outcome(outcome);
            }
        })
        .map_err(|_| {
            let mut driver = activation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .expect("failed thread spawn retains activation");
            driver.fail_before_runner();
            match driver.take_zero_process_handoff_before(cleanup_deadline) {
                Ok(handoff) => ActivationStartFailure::Teardown {
                    handoff,
                    error: HostError::Unavailable,
                },
                Err(failure) => ActivationStartFailure::Retained {
                    driver: failure.driver,
                    error: HostError::Unavailable,
                },
            }
        })?;
        *retained = Some(worker);
        match startup.recv_timeout(startup_deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                finish_failed_start(&mut retained, cleanup_deadline);
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.abort(TreeExit::Deadline);
                finish_failed_start(&mut retained, cleanup_deadline);
                match startup
                    .recv_timeout(cleanup_deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(Err(failure)) => Err(failure),
                    Ok(Ok(())) | Err(_) => {
                        Err(ActivationStartFailure::OwnedByRunner(HostError::Deadline))
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.abort(TreeExit::ParentShutdown);
                finish_failed_start(&mut retained, cleanup_deadline);
                Err(ActivationStartFailure::OwnedByRunner(
                    HostError::Unavailable,
                ))
            }
        }
    }

    pub(crate) fn abort(&self, reason: TreeExit) {
        let _ = self.control.request_termination(reason);
        self.mailbox.close();
    }

    pub(crate) fn join_before(&self, deadline: std::time::Instant) -> Result<(), HostError> {
        let mut retained = self.lock_thread();
        let Some(thread) = retained.as_mut() else {
            return Ok(());
        };
        match thread.wait(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok(()) => {
                *retained = None;
                Ok(())
            }
            Err(ThreadWaitError::Timeout) => Err(HostError::Deadline),
            Err(ThreadWaitError::Panicked) => {
                *retained = None;
                Err(HostError::Unavailable)
            }
        }
    }

    /// Final-window recovery barrier. The exact runner-owned failure state is
    /// released only after its worker is joined and native supervision proves
    /// the complete process tree is zero.
    pub(crate) fn shutdown_recovery_before(
        &self,
        deadline: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, HostError> {
        self.abort(TreeExit::ParentShutdown);
        self.join_before(deadline)?;
        let (_, token) = self
            .control
            .wait_zero_before(deadline)
            .map_err(|_| HostError::RecoveryRequired)?;
        self.recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Ok(token)
    }

    pub(crate) fn observe_exit(
        &self,
        observer: impl FnOnce(LiveRunnerExit) + Send + 'static,
    ) -> Result<(), HostError> {
        let mut observer = Some(Box::new(observer) as ExitObserver);
        let exited = {
            let mut state = self
                .exit
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*state {
                ExitState::Pending => {
                    *state = ExitState::Watching(observer.take().expect("observer retained"));
                    None
                }
                ExitState::Exited(exit) => {
                    let exit = *exit;
                    *state = ExitState::Delivered;
                    Some(exit)
                }
                ExitState::Watching(_) | ExitState::Delivered => return Err(HostError::State),
            }
        };
        if let Some(exit) = exited {
            observer.expect("observer retained")(exit);
        }
        Ok(())
    }

    /// Registers the sole service-owned observer for a newly constructed
    /// runner. The constructor guarantees `Pending`; if internal state is ever
    /// violated, deliver `Failed` instead of creating a fallible post-Ready
    /// handoff.
    pub(crate) fn observe_exit_once(&self, observer: impl FnOnce(LiveRunnerExit) + Send + 'static) {
        if let Err(error) = self.observe_exit(observer) {
            debug_assert!(false, "new live runner observer invariant: {error}");
        }
    }

    fn record_outcome(&self, outcome: RunOutcome) {
        if let Some(recovery) = outcome.recovery {
            let mut retained = self
                .recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            debug_assert!(
                retained.is_none(),
                "runner recovery is installed exactly once"
            );
            if retained.is_none() {
                *retained = Some(recovery);
            }
        }
        self.record_exit(outcome.exit);
    }

    #[cfg(test)]
    pub(crate) fn fail_for_test(&self) {
        self.record_outcome(RunOutcome::failed(()));
    }

    fn record_exit(&self, exit: LiveRunnerExit) {
        let observer = {
            let mut state = self
                .exit
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match std::mem::replace(&mut *state, ExitState::Delivered) {
                ExitState::Pending => {
                    *state = ExitState::Exited(exit);
                    None
                }
                ExitState::Watching(observer) => Some(observer),
                ExitState::Exited(_) | ExitState::Delivered => None,
            }
        };
        if let Some(observer) = observer {
            observer(exit);
        }
    }

    fn lock_thread(&self) -> std::sync::MutexGuard<'_, Option<BoundedThread>> {
        self.thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn finish_failed_start(retained: &mut Option<BoundedThread>, deadline: Instant) {
    let completed = retained.as_mut().is_some_and(|runner| {
        runner
            .wait(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    });
    if completed {
        *retained = None;
    }
}

impl WorkerDeadlineAbort for LiveRunnerHandle {
    fn request_termination(&self, reason: TreeExit) {
        self.abort(reason);
    }
}

impl Drop for LiveRunnerHandle {
    fn drop(&mut self) {
        let Some(thread) = self
            .thread
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        else {
            return;
        };
        let _ = self.control.request_termination(TreeExit::ParentShutdown);
        self.mailbox.close();
        let _ = thread.wait(cleanup_budget());
    }
}
