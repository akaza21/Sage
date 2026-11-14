use std::{
    future::Future,
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use tauri::AppHandle;

const RETRY_BUDGET: Duration = Duration::from_secs(20);
const IDLE: u8 = 0;
const RETRYING: u8 = 1;
const SETTLED: u8 = 2;
const ABANDONED: u8 = 3;

pub(super) static GIT_EXIT: GitExitCleanup = GitExitCleanup(AtomicU8::new(IDLE));

pub(super) struct GitExitCleanup(AtomicU8);

impl GitExitCleanup {
    pub(super) fn finished(&self) -> bool {
        self.0.load(Ordering::Acquire) >= SETTLED
    }

    /// An exit veto always has a continuation, even without a window to
    /// generate another close request. A completed retry cannot veto again.
    pub(super) fn defer(
        &self,
        cleanup: impl FnOnce() -> bool,
        prevent_exit: impl FnOnce(),
        schedule_retry: impl FnOnce(),
    ) -> bool {
        if self.finished() || cleanup() {
            return false;
        }
        prevent_exit();
        schedule_retry();
        true
    }

    pub(super) fn start(&'static self, app: AppHandle, code: i32) {
        if self
            .0
            .compare_exchange(IDLE, RETRYING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        tauri::async_runtime::spawn(self.retry(
            RETRY_BUDGET,
            || async {
                tauri::async_runtime::spawn_blocking(
                    crate::workspace::notify::cancel_all_workspace_watches,
                )
                .await
                .unwrap_or(false)
            },
            crate::git_process::abandon_cleanup_on_exit,
            move || app.exit(code),
        ));
    }

    async fn retry<F, Fut>(
        &self,
        budget: Duration,
        mut cleanup: F,
        abandon: impl FnOnce(),
        reissue_exit: impl FnOnce(),
    ) where
        F: FnMut() -> Fut,
        Fut: Future<Output = bool>,
    {
        let settled = tokio::time::timeout(budget, async {
            loop {
                if cleanup().await {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .is_ok();
        if !settled {
            tracing::warn!(
                ?budget,
                "windowless Git cleanup deadline expired; terminating and abandoning Sage-owned Git processes before final exit"
            );
            abandon();
        }
        self.0
            .store(if settled { SETTLED } else { ABANDONED }, Ordering::Release);
        reissue_exit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destroyed_window_delayed_cleanup_reissues_exit_within_the_bound() {
        sage_test_support::test_wait::guarded("windowless delayed cleanup", async {
            let exit = GitExitCleanup(AtomicU8::new(IDLE));
            let (scheduled, retry) = tokio::sync::oneshot::channel();
            assert!(exit.defer(
                || false,
                || {},
                || {
                    scheduled.send(()).unwrap();
                }
            ));
            retry.await.unwrap();
            let (sent, received) = tokio::sync::oneshot::channel();
            let settled = std::cell::Cell::new(false);
            tokio::time::timeout(
                Duration::from_secs(1),
                exit.retry(
                    Duration::from_millis(500),
                    || async {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        settled.set(true);
                        true
                    },
                    || panic!("settled cleanup must not be abandoned"),
                    || {
                        sent.send(settled.get()).unwrap();
                    },
                ),
            )
            .await
            .expect("delayed cleanup must reissue exit within the bound");
            assert!(received.await.unwrap(), "exit follows delayed cleanup");
            assert!(!exit.defer(
                || panic!("a completed windowless retry must not wait for Git again"),
                || panic!("the reissued exit must be allowed"),
                || panic!("the reissued exit must not schedule another retry"),
            ));
        });
    }

    #[test]
    fn permanently_blocked_cleanup_abandons_before_reissuing_final_exit() {
        sage_test_support::test_wait::guarded("windowless final teardown", async {
            let exit = GitExitCleanup(AtomicU8::new(RETRYING));
            let abandoned = std::cell::Cell::new(false);
            let (sent, received) = tokio::sync::oneshot::channel();
            tokio::time::timeout(
                Duration::from_secs(1),
                exit.retry(
                    Duration::from_millis(20),
                    std::future::pending::<bool>,
                    || abandoned.set(true),
                    || {
                        sent.send(abandoned.get()).unwrap();
                    },
                ),
            )
            .await
            .expect("permanently blocked cleanup must not strand the host");
            assert!(
                received.await.unwrap(),
                "final exit follows owned-process teardown"
            );
            assert!(!exit.defer(|| false, || panic!("final exit vetoed"), || {}));
        });
    }
}
