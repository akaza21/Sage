use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    process::Child,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, sleep, timeout},
};

use crate::error::{AppError, AppResult, GitContainmentState};

#[cfg(test)]
use super::test_support::{ForegroundObservation, ReaperTestControl, ReaperWaitMode};
use super::{GitOutput, GitProcessPolicy, GitRootAuthority, PostSpawnFailure, ReaderTasks};

const GIT_REAPER_CAPACITY: usize = 8;
const GIT_REAPER_ATTEMPTS: usize = 40;
const REAPER_WAIT_TIMEOUT: Duration = Duration::from_millis(100);
const REAPER_RETRY_DELAY: Duration = Duration::from_millis(25);
#[cfg(test)]
const TEST_RELEASE_GATE_TIMEOUT: Duration = crate::test_wait::HANG_GUARD;

pub(super) struct ContainmentRegistry {
    reaper_capacity: Arc<Semaphore>,
    unconfirmed_exits: AtomicUsize,
    uncertain_terminations: AtomicUsize,
    active_reapers: AtomicUsize,
    quarantined: Mutex<Vec<QuarantinedGitProcess>>,
}

struct QuarantinedGitProcess {
    _child: Child,
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

    #[cfg(test)]
    pub(super) fn isolated() -> Arc<Self> {
        Self::new()
    }

    #[cfg(test)]
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
    mut child: Child,
    mut readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    failure: PostSpawnFailure,
    policy: GitProcessPolicy,
    registry: Arc<ContainmentRegistry>,
    root_authority: Option<GitRootAuthority>,
) -> AppResult<GitOutput> {
    let cleanup_started = Instant::now();
    let actual_kill_succeeded = !failure.reaped && child.start_kill().is_ok();
    #[cfg(test)]
    let foreground = policy
        .reaper_control
        .as_ref()
        .map_or(ForegroundObservation::Actual, |control| control.foreground);
    #[cfg(not(test))]
    let foreground_is_actual = true;
    #[cfg(test)]
    let foreground_is_actual = foreground == ForegroundObservation::Actual;
    #[cfg(test)]
    let kill_succeeded = match foreground {
        ForegroundObservation::Actual => actual_kill_succeeded,
        ForegroundObservation::KillSucceeded => true,
        ForegroundObservation::Unproven => false,
    };
    #[cfg(not(test))]
    let kill_succeeded = actual_kill_succeeded;
    let already_reaped =
        foreground_is_actual && !kill_succeeded && matches!(child.try_wait(), Ok(Some(_)));
    let termination_proven = failure.reaped || kill_succeeded || already_reaped;
    let reaped = failure.reaped
        || already_reaped
        || (foreground_is_actual
            && match policy.reap_timeout {
                Some(deadline) => matches!(timeout(deadline, child.wait()).await, Ok(Ok(_))),
                None => false,
            });
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
    let termination_proven = termination_proven && !reaped;
    let state = if termination_proven {
        GitContainmentState::TerminatedUnreaped
    } else {
        GitContainmentState::PotentiallyRunning
    };
    let exit_charge = registry.charge(termination_proven);
    handoff_reaper(
        child,
        readers,
        reaper_lease,
        exit_charge,
        #[cfg(test)]
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

fn handoff_reaper(
    mut child: Child,
    mut readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    exit_charge: ExitCharge,
    #[cfg(test)] control: Option<Arc<ReaperTestControl>>,
    root_authority: Option<GitRootAuthority>,
) {
    let registry = Arc::clone(&exit_charge.registry);
    let active_reaper = ActiveReaper::new(&registry);
    tokio::spawn(async move {
        #[cfg(test)]
        if let Some(control) = &control
            && control.wait_mode == ReaperWaitMode::ReleaseGate
        {
            let _ = timeout(TEST_RELEASE_GATE_TIMEOUT, control.release_wait.notified()).await;
        }
        for _ in 0..GIT_REAPER_ATTEMPTS {
            let _ = child.start_kill();
            if wait_for_exit(
                &mut child,
                #[cfg(test)]
                control.as_deref(),
            )
            .await
                && readers.wait_for_eof(REAPER_WAIT_TIMEOUT).await
            {
                exit_charge.confirm();
                drop(active_reaper);
                #[cfg(test)]
                if let Some(control) = &control {
                    control.completed.notify_one();
                }
                return;
            }
            reaper_pause(
                #[cfg(test)]
                control.as_deref(),
            )
            .await;
        }
        quarantine(
            &registry,
            child,
            readers,
            reaper_lease,
            exit_charge,
            root_authority,
        );
        drop(active_reaper);
        #[cfg(test)]
        if let Some(control) = &control {
            control.completed.notify_one();
        }
    });
}

async fn wait_for_exit(
    child: &mut Child,
    #[cfg(test)] control: Option<&ReaperTestControl>,
) -> bool {
    #[cfg(test)]
    if let Some(control) = control
        && control.wait_mode == ReaperWaitMode::Exhaust
    {
        control.wait_attempts.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        return false;
    }
    matches!(timeout(REAPER_WAIT_TIMEOUT, child.wait()).await, Ok(Ok(_)))
}

async fn reaper_pause(#[cfg(test)] control: Option<&ReaperTestControl>) {
    #[cfg(test)]
    if let Some(control) = control
        && control.wait_mode == ReaperWaitMode::Exhaust
    {
        tokio::task::yield_now().await;
        return;
    }
    sleep(REAPER_RETRY_DELAY).await;
}

fn quarantine(
    registry: &Arc<ContainmentRegistry>,
    child: Child,
    readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    exit_charge: ExitCharge,
    root_authority: Option<GitRootAuthority>,
) {
    let mut owners = registry
        .quarantined
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    debug_assert!(owners.len() < GIT_REAPER_CAPACITY);
    owners.push(QuarantinedGitProcess {
        _child: child,
        _readers: readers,
        _reaper_lease: reaper_lease,
        _exit_charge: exit_charge,
        _root_authority: root_authority,
    });
}

fn decrement(counter: &AtomicUsize) {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_sub(1)
        })
        .expect("Git containment counters cannot underflow");
}

#[cfg(test)]
pub(super) async fn reap_quarantine(
    registry: &Arc<ContainmentRegistry>,
    deadline: Duration,
) -> AppResult<()> {
    let mut owners = {
        let mut quarantine = registry
            .quarantined
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *quarantine)
    };
    let expires = Instant::now() + deadline;
    let mut retained = Vec::new();
    for mut owner in owners.drain(..) {
        let remaining = expires.saturating_duration_since(Instant::now());
        if matches!(timeout(remaining, owner._child.wait()).await, Ok(Ok(_)))
            && owner
                ._readers
                .wait_for_eof(expires.saturating_duration_since(Instant::now()))
                .await
        {
            owner._exit_charge.confirm();
        } else {
            retained.push(owner);
        }
    }
    let retained_count = retained.len();
    registry
        .quarantined
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .extend(retained);
    if retained_count == 0 {
        Ok(())
    } else {
        Err(AppError::Git(format!(
            "{retained_count} quarantined Git process exits remain unconfirmed"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
