//! The background reaper: after a failed Git call returns, it keeps
//! terminating the Git tree until the root has exited, the tree is empty and
//! the inherited pipes have closed, and only then releases the call's
//! admission charge and root handles. An owner it cannot settle is
//! quarantined with everything it holds.

use super::*;

pub(super) fn handoff_reaper(
    git: GitChild,
    mut readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    exit_charge: ExitCharge,
    #[cfg(test)] control: Option<Arc<ReaperTestControl>>,
    root_authority: Option<GitRootAuthority>,
) {
    let registry = Arc::clone(&exit_charge.registry);
    let active_reaper = ActiveReaper::new(&registry);
    let GitChild {
        root: mut child,
        tree,
    } = git;
    tokio::spawn(async move {
        #[cfg(test)]
        if let Some(control) = &control
            && control.wait_mode == ReaperWaitMode::ReleaseGate
        {
            let _ = timeout(TEST_RELEASE_GATE_TIMEOUT, control.release_wait.notified()).await;
        }
        for _ in 0..GIT_REAPER_ATTEMPTS {
            let _ = tree.terminate();
            let _ = child.start_kill();
            if registry.abandoning.load(Ordering::Acquire) {
                break;
            }
            if wait_for_exit(
                &mut child,
                #[cfg(test)]
                control.as_deref(),
            )
            .await
                && wait_until_empty(&tree, REAPER_WAIT_TIMEOUT).await
                && readers.wait_for_eof(REAPER_WAIT_TIMEOUT).await
            {
                // Close the job and the root handles before reporting the
                // exit, so a confirmed exit means nothing here is still held.
                tree.close();
                drop(root_authority);
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
            GitChild { root: child, tree },
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
    git: GitChild,
    readers: ReaderTasks,
    reaper_lease: OwnedSemaphorePermit,
    exit_charge: ExitCharge,
    root_authority: Option<GitRootAuthority>,
) {
    let mut owners = registry
        .quarantined
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if registry.abandoning.load(Ordering::Acquire) {
        let _ = git.tree.terminate();
        let mut child = git.root;
        let _ = child.start_kill();
        let _ = child.try_wait();
        return;
    }
    debug_assert!(owners.len() < GIT_REAPER_CAPACITY);
    owners.push(QuarantinedGitProcess {
        _child: git.root,
        _tree: git.tree,
        _readers: readers,
        _reaper_lease: reaper_lease,
        _exit_charge: exit_charge,
        _root_authority: root_authority,
    });
}

#[cfg(test)]
pub(in super::super) async fn reap_quarantine(
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
            && wait_until_empty(
                &owner._tree,
                expires.saturating_duration_since(Instant::now()),
            )
            .await
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
