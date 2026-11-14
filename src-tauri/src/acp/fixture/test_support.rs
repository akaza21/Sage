use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::acp::process::{
    LaunchError, LaunchFailure, LaunchSpec, PlatformSupervisor, RunningTree, TreeCapacityLease,
    TreeExit,
};
use crate::acp::{
    limits::LIMITS,
    runtime::{BoundedThread, JournalHandle, JournalRunner, ThreadWaitError},
    store::AcpStore,
};

#[derive(Clone, Copy)]
pub(super) struct FixtureDeadline {
    started: Instant,
    deadline: Instant,
}

impl FixtureDeadline {
    pub(super) fn new() -> Self {
        let started = Instant::now();
        Self {
            started,
            // Bound fixture setup and scheduling with the shared test hang guard.
            deadline: started + sage_test_support::test_wait::HANG_GUARD,
        }
    }

    #[track_caller]
    pub(super) fn operation_before_cleanup(self) -> Duration {
        let remaining = self
            .operation_end_before_cleanup()
            .saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "fixture cleanup deadline reached: test hang guard elapsed {:?}, cleanup reserve {:?}",
            self.started.elapsed(),
            self.cleanup_reserve(),
        );
        remaining
    }

    pub(super) fn operation_end_before_cleanup(self) -> Instant {
        self.deadline
            .checked_sub(self.cleanup_reserve())
            .unwrap_or(self.deadline)
    }

    #[cfg(windows)]
    pub(super) fn end(self) -> Instant {
        self.deadline
    }

    #[cfg(windows)]
    pub(super) fn tree_before_cleanup(self) -> Duration {
        self.operation_before_cleanup()
            .min(Duration::from_millis(LIMITS.deadlines_ms.tree_termination))
    }

    pub(super) fn tree(self) -> Duration {
        self.remaining()
            .min(Duration::from_millis(LIMITS.deadlines_ms.tree_termination))
    }

    #[cfg(windows)]
    pub(super) fn tree_since(self, started: Instant) -> Duration {
        self.remaining().min(
            Duration::from_millis(LIMITS.deadlines_ms.tree_termination)
                .saturating_sub(started.elapsed()),
        )
    }

    pub(super) fn wal(self) -> Duration {
        self.remaining()
            .min(Duration::from_millis(LIMITS.deadlines_ms.wal_convergence))
    }

    fn remaining(self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    fn cleanup_reserve(self) -> Duration {
        // A timed-out host worker must finish before process-tree and journal
        // teardown; every observer uses the same conservative reserve.
        Duration::from_millis(
            LIMITS
                .deadlines_ms
                .tree_termination
                .saturating_mul(2)
                .saturating_add(LIMITS.deadlines_ms.wal_convergence),
        )
    }
}

#[cfg(windows)]
pub(super) fn launch_with_deadline(
    mut spec: LaunchSpec,
    lease: TreeCapacityLease,
    deadline: FixtureDeadline,
) -> Result<RunningTree, LaunchFailure> {
    let spawn_deadline = deadline.operation_end_before_cleanup();
    let spawn_ms = u64::try_from(
        spawn_deadline
            .saturating_duration_since(Instant::now())
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let terminate_ms =
        u64::try_from(deadline.tree_before_cleanup().as_millis()).unwrap_or(u64::MAX);
    spec.deadlines.spawn_ms = spec.deadlines.spawn_ms.min(spawn_ms);
    spec.deadlines.terminate_ms = spec.deadlines.terminate_ms.min(terminate_ms);
    spec.absolute_cleanup_deadline = Some(deadline.end());
    let tree = PlatformSupervisor::launch(&spec, lease)?;
    if Instant::now() < spawn_deadline {
        return Ok(tree);
    }
    let (_stdin, _stdout, control) = tree.into_parts();
    control.request_termination(TreeExit::Deadline).unwrap();
    match control.wait_zero_within(deadline.tree()) {
        Ok((_exit, token)) => Err(LaunchFailure::ZeroProcess {
            error: LaunchError::Deadline,
            token,
        }),
        Err(error) => panic!("late fixture launch did not converge: {error:?}"),
    }
}

pub(super) struct JournalCleanup {
    journal: JournalHandle,
    thread: Option<BoundedThread>,
    deadline: FixtureDeadline,
}

impl JournalCleanup {
    pub(super) fn spawn<S: AcpStore + 'static>(
        deadline: FixtureDeadline,
        journal: JournalHandle,
        runner: JournalRunner<S>,
    ) -> std::io::Result<Self> {
        let thread = BoundedThread::spawn("fixture-journal", move || {
            runner.run_blocking();
        })?;
        Ok(Self {
            journal,
            thread: Some(thread),
            deadline,
        })
    }

    pub(super) fn finish(mut self) -> Result<(), ThreadWaitError> {
        self.journal.close_when_drained();
        let result = self
            .thread
            .as_mut()
            .expect("fixture journal thread retained")
            .wait(self.deadline.wal());
        if !matches!(result, Err(ThreadWaitError::Timeout)) {
            self.thread.take();
        }
        result
    }
}

impl Drop for JournalCleanup {
    fn drop(&mut self) {
        self.journal.shutdown();
        if let Some(thread) = &mut self.thread {
            let _ = thread.wait(self.deadline.tree());
        }
    }
}
