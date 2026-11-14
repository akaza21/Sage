use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    process::Child,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, sleep, timeout},
};

use sage_core::error::{AppError, AppResult, GitContainmentState};
use sage_process::process_tree::wait_until_empty;

#[cfg(any(test, feature = "test-support"))]
use super::test_support::{ForegroundObservation, ReaperTestControl, ReaperWaitMode};
use super::{
    GitChild, GitOutput, GitProcessPolicy, GitRootAuthority, PostSpawnFailure, ProcessTree,
    ReaderTasks,
};

mod reaper;

use reaper::handoff_reaper;
#[cfg(any(test, feature = "test-support"))]
pub(super) use reaper::reap_quarantine;

const GIT_REAPER_CAPACITY: usize = 8;
const GIT_REAPER_ATTEMPTS: usize = 40;
const REAPER_WAIT_TIMEOUT: Duration = Duration::from_millis(100);
const REAPER_RETRY_DELAY: Duration = Duration::from_millis(25);
/// `sage_test_support::test_wait::HANG_GUARD`, spelled out for the
/// `test-support` feature, which cannot take sage-test-support.
#[cfg(any(test, feature = "test-support"))]
const TEST_RELEASE_GATE_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(test)]
const _: () = assert!(
    TEST_RELEASE_GATE_TIMEOUT.as_nanos() == sage_test_support::test_wait::HANG_GUARD.as_nanos()
);

pub(super) struct ContainmentRegistry {
    abandoning: AtomicBool,
    reaper_capacity: Arc<Semaphore>,
    unconfirmed_exits: AtomicUsize,
    uncertain_terminations: AtomicUsize,
    active_reapers: AtomicUsize,
    quarantined: Mutex<Vec<QuarantinedGitProcess>>,
}

struct QuarantinedGitProcess {
    _child: Child,
    _tree: ProcessTree,
    _readers: ReaderTasks,
    _reaper_lease: OwnedSemaphorePermit,
    _exit_charge: ExitCharge,
    _root_authority: Option<GitRootAuthority>,
}

struct ExitCharge {
    registry: Arc<ContainmentRegistry>,
    uncertain_termination: bool,
}

struct ActiveReaper {
    registry: Arc<ContainmentRegistry>,
}

impl ContainmentRegistry {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            abandoning: AtomicBool::new(false),
            reaper_capacity: Arc::new(Semaphore::new(GIT_REAPER_CAPACITY)),
            unconfirmed_exits: AtomicUsize::new(0),
            uncertain_terminations: AtomicUsize::new(0),
            active_reapers: AtomicUsize::new(0),
            quarantined: Mutex::new(Vec::with_capacity(GIT_REAPER_CAPACITY)),
        })
    }

    fn charge(self: &Arc<Self>, termination_proven: bool) -> ExitCharge {
        self.unconfirmed_exits.fetch_add(1, Ordering::SeqCst);
        if !termination_proven {
            self.uncertain_terminations.fetch_add(1, Ordering::SeqCst);
        }
        ExitCharge {
            registry: Arc::clone(self),
            uncertain_termination: !termination_proven,
        }
    }

    fn admission_state(&self) -> GitContainmentState {
        if self.uncertain_terminations.load(Ordering::SeqCst) == 0 {
            GitContainmentState::TerminatedUnreaped
        } else {
            GitContainmentState::PotentiallyRunning
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn isolated() -> Arc<Self> {
        Self::new()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn observation(&self) -> (usize, usize, usize, usize) {
        (
            self.unconfirmed_exits.load(Ordering::SeqCst),
            self.quarantined
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            self.active_reapers.load(Ordering::SeqCst),
            self.reaper_capacity.available_permits(),
        )
    }
}

impl ExitCharge {
    fn confirm(self) {
        decrement(&self.registry.unconfirmed_exits);
        if self.uncertain_termination {
            decrement(&self.registry.uncertain_terminations);
        }
    }
}

impl ActiveReaper {
    fn new(registry: &Arc<ContainmentRegistry>) -> Self {
        registry.active_reapers.fetch_add(1, Ordering::SeqCst);
        Self {
            registry: Arc::clone(registry),
        }
    }
}

impl Drop for ActiveReaper {
    fn drop(&mut self) {
        decrement(&self.registry.active_reapers);
    }
}

pub(super) async fn cleanup_failure(
    git: GitChild,
    mut readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    failure: PostSpawnFailure,
    policy: GitProcessPolicy,
    registry: Arc<ContainmentRegistry>,
    root_authority: Option<GitRootAuthority>,
) -> AppResult<GitOutput> {
    let GitChild {
        root: mut child,
        tree,
    } = git;
    let cleanup_started = Instant::now();
    // Terminating the tree also ends the root, and whatever the root started.
    let tree_terminated = tree.terminate();
    let actual_kill_succeeded = tree_terminated || (!failure.reaped && child.start_kill().is_ok());
    #[cfg(any(test, feature = "test-support"))]
    let foreground = policy
        .reaper_control
        .as_ref()
        .map_or(ForegroundObservation::Actual, |control| control.foreground);
    #[cfg(not(any(test, feature = "test-support")))]
    let foreground_is_actual = true;
    #[cfg(any(test, feature = "test-support"))]
    let foreground_is_actual = foreground == ForegroundObservation::Actual;
    #[cfg(any(test, feature = "test-support"))]
    let kill_succeeded = match foreground {
        ForegroundObservation::Actual => actual_kill_succeeded,
        ForegroundObservation::KillSucceeded => true,
        ForegroundObservation::Unproven => false,
    };
    #[cfg(not(any(test, feature = "test-support")))]
    let kill_succeeded = actual_kill_succeeded;
    let already_reaped =
        foreground_is_actual && !kill_succeeded && matches!(child.try_wait(), Ok(Some(_)));
    let termination_proven = failure.reaped || kill_succeeded || already_reaped;
    let root_reaped = failure.reaped
        || already_reaped
        || (foreground_is_actual
            && match policy.reap_timeout {
                Some(deadline) => matches!(timeout(deadline, child.wait()).await, Ok(Ok(_))),
                None => false,
            });
    // The root's exit proves nothing about what it started: the tree must be
    // empty as well.
    let reaped = root_reaped
        && wait_until_empty(
            &tree,
            policy
                .reap_timeout
                .unwrap_or(Duration::ZERO)
                .saturating_sub(cleanup_started.elapsed()),
        )
        .await;
    let settled = reaped
        && readers
            .wait_for_eof(
                policy
                    .reap_timeout
                    .unwrap_or(Duration::ZERO)
                    .saturating_sub(cleanup_started.elapsed()),
            )
            .await;
    if settled {
        return Err(settled_failure(
            policy.visible_effect,
            format!(
                "{}; the Git process was terminated and reaped",
                failure.message
            ),
        ));
    }

    // A reaped proxy with open inherited streams still has an unconfirmed
    // backend. Keep both its reader ownership and mutation admission charged.
    // A terminated tree has no such backend: its termination stays proven.
    let termination_proven = termination_proven && (!reaped || tree_terminated);
    let state = if termination_proven {
        GitContainmentState::TerminatedUnreaped
    } else {
        GitContainmentState::PotentiallyRunning
    };
    let exit_charge = registry.charge(termination_proven);
    handoff_reaper(
        GitChild { root: child, tree },
        readers,
        reaper_lease,
        exit_charge,
        #[cfg(any(test, feature = "test-support"))]
        policy.reaper_control,
        root_authority,
    );
    Err(containment(
        state,
        if reaped {
            format!(
                "{}; the Git root exited, but its inherited output streams have not settled",
                failure.message
            )
        } else if termination_proven {
            format!(
                "{}; Git termination is proven, but process exit is not confirmed",
                failure.message
            )
        } else {
            format!(
                "{}; Git termination and process exit could not be confirmed",
                failure.message
            )
        },
    ))
}

fn settled_failure(visible_effect: bool, message: String) -> AppError {
    if visible_effect {
        AppError::GitEffectUnknown(message)
    } else {
        AppError::Git(message)
    }
}

pub(super) async fn reserve_reaper_owner(
    registry: Arc<ContainmentRegistry>,
    admission_timeout: Duration,
) -> AppResult<OwnedSemaphorePermit> {
    timeout(
        admission_timeout,
        Arc::clone(&registry.reaper_capacity).acquire_owned(),
    )
    .await
    .map_err(|_| AppError::ResourceLimit("the bounded Git reaper is at capacity".into()))?
    .map_err(|_| AppError::Git("the bounded Git reaper is unavailable".into()))
}

pub(super) fn git_containment() -> Arc<ContainmentRegistry> {
    static GIT_CONTAINMENT: OnceLock<Arc<ContainmentRegistry>> = OnceLock::new();
    GIT_CONTAINMENT
        .get_or_init(ContainmentRegistry::new)
        .clone()
}

/// Final host teardown only: retain no quarantined owners after making one
/// last kill attempt. Live owners keep their Jobs until host handle closure;
/// reapers that finish concurrently see `abandoning` instead of quarantining.
pub fn abandon_cleanup_on_exit() {
    abandon(&git_containment());
}

fn abandon(registry: &ContainmentRegistry) {
    registry.abandoning.store(true, Ordering::Release);
    registry.reaper_capacity.close();
    let mut owners = registry
        .quarantined
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for mut owner in owners.drain(..) {
        let _ = owner._tree.terminate();
        let _ = owner._child.start_kill();
        let _ = owner._child.try_wait();
        // Dropping the tree closes its owned kill-on-close Job on Windows;
        // dropping readers aborts only this process's bounded pipe tasks.
    }
}

pub(super) async fn await_mutation_admission(
    registry: &ContainmentRegistry,
    admission_timeout: Duration,
) -> AppResult<()> {
    timeout(admission_timeout, async {
        while registry.unconfirmed_exits.load(Ordering::SeqCst) != 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| {
        containment(
            registry.admission_state(),
            "a prior Git process exit is still unconfirmed",
        )
    })
}

fn containment(state: GitContainmentState, message: impl Into<String>) -> AppError {
    AppError::GitContainment {
        state,
        message: message.into(),
    }
}

fn decrement(counter: &AtomicUsize) {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_sub(1)
        })
        .expect("Git containment counters cannot underflow");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_teardown_releases_owned_quarantine_and_refuses_further_git() {
        sage_test_support::test_wait::guarded("final Git teardown", async {
            let harness = crate::process::GitProcessTestHarness::new();
            let (policy, reaper) = harness.exhausting_reaper_policy(crate::budget::READ);
            let registry = policy.containment_registry.clone().unwrap();
            let root = sage_test_support::test_paths::temp_dir();
            let result = crate::process::run(&root, &["--version"], 1024, policy).await;
            assert!(matches!(result, Err(AppError::GitContainment { .. })));
            reaper
                .wait_for_quarantine(sage_test_support::test_wait::HANG_GUARD)
                .await
                .unwrap();
            assert_eq!(harness.observation().quarantined_owners, 1);

            // A process outside this registry must remain untouched.
            let mut unrelated = crate::process::fixture_git()
                .args(["hash-object", "--stdin"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            abandon(&registry);
            let unrelated_alive = unrelated.try_wait().unwrap().is_none();
            let _ = unrelated.kill();
            unrelated.wait().unwrap();
            assert!(unrelated_alive);
            assert_eq!(harness.observation().quarantined_owners, 0);
            assert!(
                harness
                    .probe_reaper_owner(Duration::from_millis(10))
                    .await
                    .is_err()
            );
        });
    }

    #[test]
    fn reaper_owner_capacity_accepts_exact_eight_rejects_plus_one_and_resets() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let registry = ContainmentRegistry::isolated();
            let mut first = Vec::new();
            for _ in 0..GIT_REAPER_CAPACITY {
                first.push(
                    reserve_reaper_owner(Arc::clone(&registry), Duration::from_millis(50))
                        .await
                        .unwrap(),
                );
            }
            assert_eq!(registry.reaper_capacity.available_permits(), 0);
            assert!(matches!(
                reserve_reaper_owner(Arc::clone(&registry), Duration::from_millis(10)).await,
                Err(AppError::ResourceLimit(_))
            ));

            drop(first);
            assert_eq!(
                registry.reaper_capacity.available_permits(),
                GIT_REAPER_CAPACITY
            );
            let mut second = Vec::new();
            for _ in 0..GIT_REAPER_CAPACITY {
                second.push(
                    reserve_reaper_owner(Arc::clone(&registry), Duration::from_millis(50))
                        .await
                        .unwrap(),
                );
            }
            assert!(matches!(
                reserve_reaper_owner(Arc::clone(&registry), Duration::from_millis(10)).await,
                Err(AppError::ResourceLimit(_))
            ));
            drop(second);
            assert_eq!(
                registry.reaper_capacity.available_permits(),
                GIT_REAPER_CAPACITY
            );
        });
    }
}
