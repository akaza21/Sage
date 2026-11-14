use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{Notify, oneshot},
    time::timeout,
};

use sage_core::error::{AppError, AppResult};

use super::{
    GitProcessPolicy,
    containment::{self, ContainmentRegistry},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ForegroundObservation {
    Actual,
    KillSucceeded,
    Unproven,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReaperWaitMode {
    ReleaseGate,
    Exhaust,
}

pub(super) struct ReaperTestControl {
    pub(super) foreground: ForegroundObservation,
    pub(super) wait_mode: ReaperWaitMode,
    pub(super) release_wait: Notify,
    pub(super) completed: Notify,
    pub(super) wait_attempts: AtomicUsize,
}

impl ReaperTestControl {
    fn new(foreground: ForegroundObservation, wait_mode: ReaperWaitMode) -> Arc<Self> {
        Arc::new(Self {
            foreground,
            wait_mode,
            release_wait: Notify::new(),
            completed: Notify::new(),
            wait_attempts: AtomicUsize::new(0),
        })
    }

    fn release_wait(&self) {
        self.release_wait.notify_one();
    }

    async fn wait_for_completion(&self, deadline: Duration) -> AppResult<()> {
        timeout(deadline, self.completed.notified())
            .await
            .map_err(|_| AppError::Git("the injected Git reaper did not finish in time".into()))
    }

    fn wait_attempts(&self) -> usize {
        self.wait_attempts.load(Ordering::SeqCst)
    }
}

pub(crate) struct GitProcessTestHarness {
    registry: Arc<ContainmentRegistry>,
}

pub(crate) struct GitReaperTestHandle {
    registry: Arc<ContainmentRegistry>,
    control: Arc<ReaperTestControl>,
}

pub(crate) struct GitContainmentTestObservation {
    pub(crate) unconfirmed_exits: usize,
    pub(crate) quarantined_owners: usize,
    pub(crate) active_reapers: usize,
    pub(crate) available_reaper_permits: usize,
}

pub(crate) struct DeadlineWaitGate {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

pub(crate) struct DeadlineWaitGateHandle {
    entered: Option<oneshot::Receiver<()>>,
    release: Option<oneshot::Sender<()>>,
}

impl DeadlineWaitGate {
    fn new() -> (Arc<Self>, DeadlineWaitGateHandle) {
        let (entered_sender, entered) = oneshot::channel();
        let (release, release_receiver) = oneshot::channel();
        (
            Arc::new(Self {
                entered: Mutex::new(Some(entered_sender)),
                release: Mutex::new(Some(release_receiver)),
            }),
            DeadlineWaitGateHandle {
                entered: Some(entered),
                release: Some(release),
            },
        )
    }

    pub(super) async fn wait(&self) {
        let entered = self
            .entered
            .lock()
            .expect("deadline gate entry lock poisoned")
            .take()
            .expect("deadline gate entered more than once");
        let release = self
            .release
            .lock()
            .expect("deadline gate release lock poisoned")
            .take()
            .expect("deadline gate waited more than once");
        let _ = entered.send(());
        let _ = release.await;
    }
}

impl DeadlineWaitGateHandle {
    pub(crate) async fn wait_for_entry(&mut self) -> AppResult<()> {
        let entered = self
            .entered
            .take()
            .ok_or_else(|| AppError::Git("the Git deadline gate was awaited twice".into()))?;
        entered
            .await
            .map_err(|_| AppError::Git("the Git deadline gate closed before entry".into()))
    }

    pub(crate) fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

impl GitProcessPolicy {
    pub(crate) fn with_deadline_wait_gate(
        command_timeout: Duration,
    ) -> (Self, DeadlineWaitGateHandle) {
        let (deadline_wait_gate, handle) = DeadlineWaitGate::new();
        (
            Self {
                deadline_wait_gate: Some(deadline_wait_gate),
                ..Self::mutation(command_timeout)
            },
            handle,
        )
    }

    /// Records Git's trace2 events, from this call and every Git it starts.
    pub(crate) fn with_trace2_event(mut self, target: std::path::PathBuf) -> Self {
        self.trace2_event = Some(target);
        self
    }

    pub(crate) fn nonconvergent_reap(command_timeout: Duration) -> Self {
        Self {
            reap_timeout: None,
            ..Self::mutation(command_timeout)
        }
    }

    pub(crate) fn post_reap_failure(command_timeout: Duration) -> Self {
        Self {
            stdin: None,
            command_timeout,
            admission_timeout: crate::git_budget::ADMISSION,
            reap_timeout: Some(crate::git_budget::REAP),
            mutation: true,
            visible_effect: true,
            post_reap_failure: true,
            post_spawn_failure: false,
            stdout_reader_failure: false,
            reaper_control: None,
            containment_registry: None,
            before_spawn_hook: None,
            after_process_hook: None,
            spawned_hook: None,
            deadline_wait_gate: None,
            trace2_event: None,
        }
    }

    pub(crate) fn pre_ref_post_reap_failure(command_timeout: Duration) -> Self {
        let mut policy = Self::post_reap_failure(command_timeout);
        policy.visible_effect = false;
        policy
    }

    pub(crate) fn post_spawn_failure(command_timeout: Duration) -> Self {
        Self {
            stdin: None,
            command_timeout,
            admission_timeout: crate::git_budget::ADMISSION,
            reap_timeout: Some(crate::git_budget::REAP),
            mutation: true,
            visible_effect: true,
            post_reap_failure: false,
            post_spawn_failure: true,
            stdout_reader_failure: false,
            reaper_control: None,
            containment_registry: None,
            before_spawn_hook: None,
            after_process_hook: None,
            spawned_hook: None,
            deadline_wait_gate: None,
            trace2_event: None,
        }
    }

    pub(crate) fn stdout_reader_failure(command_timeout: Duration) -> Self {
        Self {
            stdin: None,
            command_timeout,
            admission_timeout: crate::git_budget::ADMISSION,
            reap_timeout: Some(crate::git_budget::REAP),
            mutation: true,
            visible_effect: true,
            post_reap_failure: false,
            post_spawn_failure: false,
            stdout_reader_failure: true,
            reaper_control: None,
            containment_registry: None,
            before_spawn_hook: None,
            after_process_hook: None,
            spawned_hook: None,
            deadline_wait_gate: None,
            trace2_event: None,
        }
    }

    pub(crate) fn with_before_spawn_hook(
        mut self,
        hook: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.before_spawn_hook = Some(Arc::new(hook));
        self
    }

    pub(crate) fn with_after_process_hook(
        mut self,
        hook: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.after_process_hook = Some(Arc::new(hook));
        self
    }

    /// Receives the Git child's process id right after it spawns, so a test
    /// can observe the child's lifecycle from outside the code under test.
    pub(crate) fn with_spawned_hook(mut self, hook: impl Fn(u32) + Send + Sync + 'static) -> Self {
        self.spawned_hook = Some(Arc::new(hook));
        self
    }

    pub(crate) fn potentially_running(command_timeout: Duration) -> (Self, GitReaperTestHandle) {
        GitProcessTestHarness::new().controlled_policy(
            command_timeout,
            ForegroundObservation::Unproven,
            ReaperWaitMode::ReleaseGate,
        )
    }

    pub(crate) fn terminated_unreaped_withheld_wait(
        command_timeout: Duration,
    ) -> (Self, GitReaperTestHandle) {
        GitProcessTestHarness::new().controlled_policy(
            command_timeout,
            ForegroundObservation::KillSucceeded,
            ReaperWaitMode::ReleaseGate,
        )
    }
}

impl GitProcessTestHarness {
    pub(crate) fn new() -> Self {
        Self {
            registry: ContainmentRegistry::isolated(),
        }
    }

    pub(crate) fn exhausting_reaper_policy(
        &self,
        command_timeout: Duration,
    ) -> (GitProcessPolicy, GitReaperTestHandle) {
        let (mut policy, handle) = self.controlled_policy(
            command_timeout,
            ForegroundObservation::KillSucceeded,
            ReaperWaitMode::Exhaust,
        );
        policy.mutation = false;
        policy.visible_effect = false;
        (policy, handle)
    }

    pub(crate) fn mutation_policy(
        &self,
        command_timeout: Duration,
        admission_timeout: Duration,
    ) -> GitProcessPolicy {
        GitProcessPolicy {
            admission_timeout,
            containment_registry: Some(Arc::clone(&self.registry)),
            ..GitProcessPolicy::mutation(command_timeout)
        }
    }

    /// [`GitProcessPolicy::nonconvergent_reap`] charged to this harness, so a
    /// test can observe when the background reaper confirms the exit.
    pub(crate) fn nonconvergent_reap_policy(&self, command_timeout: Duration) -> GitProcessPolicy {
        GitProcessPolicy {
            containment_registry: Some(Arc::clone(&self.registry)),
            ..GitProcessPolicy::nonconvergent_reap(command_timeout)
        }
    }

    pub(crate) fn observation(&self) -> GitContainmentTestObservation {
        let (unconfirmed_exits, quarantined_owners, active_reapers, available_reaper_permits) =
            self.registry.observation();
        GitContainmentTestObservation {
            unconfirmed_exits,
            quarantined_owners,
            active_reapers,
            available_reaper_permits,
        }
    }

    pub(crate) async fn probe_reaper_owner(&self, deadline: Duration) -> AppResult<()> {
        containment::reserve_reaper_owner(Arc::clone(&self.registry), deadline)
            .await
            .map(drop)
    }

    pub(crate) async fn reap_quarantine(&self, deadline: Duration) -> AppResult<()> {
        containment::reap_quarantine(&self.registry, deadline).await
    }

    fn controlled_policy(
        &self,
        command_timeout: Duration,
        foreground: ForegroundObservation,
        wait_mode: ReaperWaitMode,
    ) -> (GitProcessPolicy, GitReaperTestHandle) {
        let control = ReaperTestControl::new(foreground, wait_mode);
        (
            GitProcessPolicy {
                stdin: None,
                command_timeout,
                admission_timeout: crate::git_budget::ADMISSION,
                reap_timeout: None,
                mutation: true,
                visible_effect: true,
                post_reap_failure: false,
                post_spawn_failure: true,
                stdout_reader_failure: false,
                reaper_control: Some(Arc::clone(&control)),
                containment_registry: Some(Arc::clone(&self.registry)),
                before_spawn_hook: None,
                after_process_hook: None,
                spawned_hook: None,
                deadline_wait_gate: None,
                trace2_event: None,
            },
            GitReaperTestHandle {
                registry: Arc::clone(&self.registry),
                control,
            },
        )
    }
}

impl GitReaperTestHandle {
    pub(crate) fn release(&self) {
        self.release_wait();
    }

    pub(crate) fn release_wait(&self) {
        self.control.release_wait();
    }

    pub(crate) async fn wait_for_exit(&self, deadline: Duration) -> AppResult<()> {
        self.control.wait_for_completion(deadline).await
    }

    pub(crate) async fn wait_for_quarantine(&self, deadline: Duration) -> AppResult<()> {
        self.control.wait_for_completion(deadline).await
    }

    pub(crate) fn wait_attempts(&self) -> usize {
        self.control.wait_attempts()
    }

    pub(crate) fn mutation_policy(
        &self,
        command_timeout: Duration,
        admission_timeout: Duration,
    ) -> GitProcessPolicy {
        GitProcessTestHarness {
            registry: Arc::clone(&self.registry),
        }
        .mutation_policy(command_timeout, admission_timeout)
    }
}
