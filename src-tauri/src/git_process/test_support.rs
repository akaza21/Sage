use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{sync::Notify, time::timeout};

use crate::error::{AppError, AppResult};

use super::{
    GIT_ADMISSION_TIMEOUT, GIT_REAP_TIMEOUT, GitProcessPolicy,
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

impl GitProcessPolicy {
    pub(crate) fn nonconvergent_reap(command_timeout: Duration) -> Self {
        Self {
            reap_timeout: None,
            ..Self::mutation(command_timeout)
        }
    }

    pub(crate) fn post_reap_failure(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            visible_effect: true,
            post_reap_failure: true,
            stdout_reader_failure: false,
            reaper_control: None,
            containment_registry: None,
            before_spawn_hook: None,
            after_process_hook: None,
        }
    }

    pub(crate) fn pre_ref_post_reap_failure(command_timeout: Duration) -> Self {
        let mut policy = Self::post_reap_failure(command_timeout);
        policy.visible_effect = false;
        policy
    }

    pub(crate) fn stdout_reader_failure(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            visible_effect: true,
            post_reap_failure: false,
            stdout_reader_failure: true,
            reaper_control: None,
            containment_registry: None,
            before_spawn_hook: None,
            after_process_hook: None,
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
                command_timeout,
                admission_timeout: GIT_ADMISSION_TIMEOUT,
                reap_timeout: None,
                mutation: true,
                visible_effect: true,
                post_reap_failure: false,
                stdout_reader_failure: false,
                reaper_control: Some(Arc::clone(&control)),
                containment_registry: Some(Arc::clone(&self.registry)),
                before_spawn_hook: None,
                after_process_hook: None,
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
