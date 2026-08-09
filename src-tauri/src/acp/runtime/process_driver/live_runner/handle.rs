use std::{
    sync::{Arc, Mutex, Weak, mpsc},
    time::Instant,
};

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    process::{StdoutEndpoint, TreeControl, TreeExit},
    runtime::{
        ActivationProcessDriver, ActivationTransport, BoundedThread, CoordinatorActivationJournal,
        HostError, ProjectIntentAuthority, RuntimeHost, ThreadWaitError,
    },
};

use super::{cleanup_budget, run};
use crate::acp::runtime::process_driver::{
    DriverMailbox, FirstTurnWorkerContext, StdinTransport, worker_command::WorkerDeadlineAbort,
};

pub(crate) struct LiveRunnerHandle {
    mailbox: Arc<DriverMailbox>,
    control: TreeControl,
    thread: Mutex<Option<BoundedThread>>,
    exit: Mutex<ExitState>,
}

pub(super) struct RunInput<Auth>
where
    Auth: ProjectIntentAuthority,
{
    pub(super) context: FirstTurnWorkerContext<Auth>,
    pub(super) stdout: StdoutEndpoint,
    pub(super) host: Weak<RuntimeHost>,
    pub(super) mailbox: Arc<DriverMailbox>,
    pub(super) control: TreeControl,
    pub(super) ready: mpsc::SyncSender<Result<(), HostError>>,
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
        })
    }

    pub(crate) fn mailbox(&self) -> Arc<DriverMailbox> {
        Arc::clone(&self.mailbox)
    }

    pub(crate) fn start_activation<A, R, T, Auth>(
        self: &Arc<Self>,
        activation: ActivationProcessDriver<A, CoordinatorActivationJournal, R, T, TreeControl>,
        context: FirstTurnWorkerContext<Auth>,
        stdout: StdoutEndpoint,
        host: Weak<RuntimeHost>,
        startup_deadline: Instant,
        cleanup_deadline: Instant,
    ) -> Result<(), HostError>
    where
        A: HarnessAdapter + Send + 'static,
        R: ResourceMapper + Send + 'static,
        T: StdinTransport + ActivationTransport + Send + 'static,
        Auth: ProjectIntentAuthority + 'static,
    {
        if startup_deadline > cleanup_deadline || Instant::now() >= startup_deadline {
            self.abort(TreeExit::Deadline);
            let _ = self.control.wait_before(cleanup_deadline);
            return Err(HostError::Deadline);
        }
        let mut retained = self.lock_thread();
        if retained.is_some() {
            return Err(HostError::State);
        }
        let (ready, startup) = mpsc::sync_channel(1);
        let mailbox = Arc::clone(&self.mailbox);
        let control = self.control.clone();
        let runner = Arc::downgrade(self);
        let worker = BoundedThread::spawn("sage-acp-live-runner", move || {
            let exit = run(
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
                runner.record_exit(exit);
            }
        })
        .map_err(|_| {
            self.abort(TreeExit::ParentShutdown);
            let _ = self.control.wait_before(cleanup_deadline);
            HostError::Unavailable
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
                Err(HostError::Deadline)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.abort(TreeExit::ParentShutdown);
                finish_failed_start(&mut retained, cleanup_deadline);
                Err(HostError::Unavailable)
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
