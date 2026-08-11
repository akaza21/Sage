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
    time::{sleep, timeout},
};

use crate::error::{AppError, AppResult, GitContainmentState};

use super::{GitOutput, GitProcessPolicy, PostSpawnFailure, ReaderTasks};

const GIT_REAPER_CAPACITY: usize = 8;
const GIT_REAPER_ATTEMPTS: usize = 40;
const IO_CLEANUP_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(test)]
const INJECTED_REAPER_RELEASE_TIMEOUT: Duration = Duration::from_secs(1);

struct QuarantinedGitProcess {
    _child: Child,
    _reaper_lease: OwnedSemaphorePermit,
}

pub(super) async fn cleanup_failure(
    mut child: Child,
    readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    failure: PostSpawnFailure,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    if failure.reaped {
        settle_readers(readers).await;
        return Err(AppError::GitEffectUnknown(format!(
            "{}; the Git process exited and was reaped",
            failure.message
        )));
    }

    #[cfg(test)]
    let injected_unproven = policy.potentially_running.is_some();
    #[cfg(not(test))]
    let injected_unproven = false;
    let kill_succeeded = child.start_kill().is_ok() && !injected_unproven;
    let already_reaped =
        !injected_unproven && !kill_succeeded && matches!(child.try_wait(), Ok(Some(_)));
    let termination_proven = kill_succeeded || already_reaped;
    let reaped = already_reaped
        || (match policy.reap_timeout {
            Some(deadline) => matches!(timeout(deadline, child.wait()).await, Ok(Ok(_))),
            None => false,
        } && !injected_unproven);
    if reaped {
        settle_readers(readers).await;
        return Err(AppError::GitEffectUnknown(format!(
            "{}; the Git process was terminated and reaped",
            failure.message
        )));
    }

    let state = if termination_proven {
        GitContainmentState::TerminatedUnreaped
    } else {
        GitContainmentState::PotentiallyRunning
    };
    if !termination_proven {
        potentially_running_git().fetch_add(1, Ordering::SeqCst);
    }
    handoff_reaper(
        child,
        readers,
        reaper_lease,
        termination_proven,
        #[cfg(test)]
        policy.potentially_running,
    );
    Err(containment(
        state,
        if termination_proven {
            format!(
                "{}; Git termination is proven, but reap did not converge",
                failure.message
            )
        } else {
            format!(
                "{}; Git termination and reap could not be confirmed",
                failure.message
            )
        },
    ))
}

pub(super) async fn reserve_reaper_owner(
    capacity: Arc<Semaphore>,
    admission_timeout: Duration,
) -> AppResult<OwnedSemaphorePermit> {
    timeout(admission_timeout, capacity.acquire_owned())
        .await
        .map_err(|_| AppError::ResourceLimit("the bounded Git reaper is at capacity".into()))?
        .map_err(|_| AppError::Git("the bounded Git reaper is unavailable".into()))
}

pub(super) fn git_reaper_capacity() -> Arc<Semaphore> {
    static GIT_REAPER: OnceLock<Arc<Semaphore>> = OnceLock::new();
    GIT_REAPER
        .get_or_init(|| Arc::new(Semaphore::new(GIT_REAPER_CAPACITY)))
        .clone()
}

pub(super) async fn await_mutation_admission(admission_timeout: Duration) -> AppResult<()> {
    timeout(admission_timeout, async {
        while potentially_running_git().load(Ordering::SeqCst) != 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| {
        containment(
            GitContainmentState::PotentiallyRunning,
            "a prior Git process is still under containment",
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
    readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    mut termination_proven: bool,
    #[cfg(test)] control: Option<super::PotentiallyRunningControl>,
) {
    tokio::spawn(async move {
        settle_readers(readers).await;
        #[cfg(test)]
        if let Some(control) = &control {
            let _ = timeout(INJECTED_REAPER_RELEASE_TIMEOUT, control.release.notified()).await;
        }
        for _ in 0..GIT_REAPER_ATTEMPTS {
            if !termination_proven {
                termination_proven = child.start_kill().is_ok();
                if termination_proven {
                    potentially_running_git().fetch_sub(1, Ordering::SeqCst);
                }
            }
            if matches!(
                timeout(Duration::from_millis(100), child.wait()).await,
                Ok(Ok(_))
            ) {
                if !termination_proven {
                    potentially_running_git().fetch_sub(1, Ordering::SeqCst);
                }
                #[cfg(test)]
                if let Some(control) = &control {
                    control.exited.notify_one();
                }
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }
        quarantine(child, reaper_lease);
    });
}

async fn settle_readers(readers: ReaderTasks) {
    if let Some(task) = &readers.stdout {
        task.abort();
    }
    if let Some(task) = &readers.stderr {
        task.abort();
    }
    let _ = timeout(IO_CLEANUP_TIMEOUT, async {
        if let Some(task) = readers.stdout {
            let _ = task.await;
        }
        if let Some(task) = readers.stderr {
            let _ = task.await;
        }
    })
    .await;
}

fn quarantine(child: Child, reaper_lease: OwnedSemaphorePermit) {
    let mut owners = quarantined_git()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    debug_assert!(owners.len() < GIT_REAPER_CAPACITY);
    owners.push(QuarantinedGitProcess {
        _child: child,
        _reaper_lease: reaper_lease,
    });
}

fn potentially_running_git() -> &'static AtomicUsize {
    static POTENTIALLY_RUNNING_GIT: AtomicUsize = AtomicUsize::new(0);
    &POTENTIALLY_RUNNING_GIT
}

fn quarantined_git() -> &'static Mutex<Vec<QuarantinedGitProcess>> {
    static QUARANTINED_GIT: OnceLock<Mutex<Vec<QuarantinedGitProcess>>> = OnceLock::new();
    QUARANTINED_GIT.get_or_init(|| Mutex::new(Vec::with_capacity(GIT_REAPER_CAPACITY)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaper_owner_capacity_accepts_exact_eight_rejects_plus_one_and_resets() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let capacity = Arc::new(Semaphore::new(GIT_REAPER_CAPACITY));
            let mut first = Vec::new();
            for _ in 0..GIT_REAPER_CAPACITY {
                first.push(
                    reserve_reaper_owner(Arc::clone(&capacity), Duration::from_millis(50))
                        .await
                        .unwrap(),
                );
            }
            assert_eq!(capacity.available_permits(), 0);
            assert!(matches!(
                reserve_reaper_owner(Arc::clone(&capacity), Duration::from_millis(10)).await,
                Err(AppError::ResourceLimit(_))
            ));

            drop(first);
            assert_eq!(capacity.available_permits(), GIT_REAPER_CAPACITY);
            let mut second = Vec::new();
            for _ in 0..GIT_REAPER_CAPACITY {
                second.push(
                    reserve_reaper_owner(Arc::clone(&capacity), Duration::from_millis(50))
                        .await
                        .unwrap(),
                );
            }
            assert!(matches!(
                reserve_reaper_owner(Arc::clone(&capacity), Duration::from_millis(10)).await,
                Err(AppError::ResourceLimit(_))
            ));
            drop(second);
            assert_eq!(capacity.available_permits(), GIT_REAPER_CAPACITY);
        });
    }
}
