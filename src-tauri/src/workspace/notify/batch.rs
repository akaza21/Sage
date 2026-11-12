//! Coalesces a watch's notices into one WebView event per short window.
//!
//! Watcher threads push accepted notices; one emitter thread per watch
//! sends them as a single batch once the window that the first notice opened
//! has passed. Within a batch a path appears once, with its latest kind. A
//! batch never holds more than [`MAX_BATCH_NOTICES`] notices: past that, the
//! root that overflowed is sent as one resync notice in place of its paths,
//! as a lost watcher queue is, and the WebView reloads that root's listings.

use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Serialize;

use super::{OVERFLOW_RELATIVE_PATH, WorkspaceNotifyNotice};

/// How long the first notice of a batch waits for others. The WebView's Git
/// refresh settles for 300 ms anyway, so 100 ms adds no visible delay while
/// capping a build's writes at ten events a second.
pub(super) const NOTICE_BATCH_WINDOW: Duration = Duration::from_millis(100);
/// The most notices one event carries. A folder refresh per notice is the
/// WebView's cost, and a root with more changes than this in 100 ms is
/// cheaper to resync once.
pub(super) const MAX_BATCH_NOTICES: usize = 256;
/// How often an idle emitter checks whether its watch was cancelled, which
/// matches the watcher threads' own wait.
const CANCEL_CHECK: Duration = Duration::from_millis(100);

const RESYNC_KIND: &str = "resync";

/// The payload of one WebView event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceNotifyBatch {
    pub notices: Vec<WorkspaceNotifyNotice>,
}

/// Notices gathered since the last event.
#[derive(Debug, Default)]
pub(super) struct NoticeBatch {
    notices: Vec<WorkspaceNotifyNotice>,
    /// Each queued notice's index, by root and path.
    index: HashMap<(String, String), usize>,
    /// The resync notice of each root that overflowed or lost records. Its
    /// paths are dropped, since the resync reloads every listing they touch.
    resyncs: Vec<WorkspaceNotifyNotice>,
}

impl NoticeBatch {
    pub(super) fn is_empty(&self) -> bool {
        self.notices.is_empty() && self.resyncs.is_empty()
    }

    pub(super) fn push(&mut self, notice: WorkspaceNotifyNotice) {
        if self.resyncing(&notice.root_id) {
            return;
        }
        if notice.kind == RESYNC_KIND {
            self.resync(notice);
            return;
        }
        let key = (notice.root_id.clone(), notice.relative_path.clone());
        if let Some(&slot) = self.index.get(&key) {
            self.notices[slot].kind = notice.kind;
            return;
        }
        if self.notices.len() >= MAX_BATCH_NOTICES {
            self.resync(notice);
            return;
        }
        self.index.insert(key, self.notices.len());
        self.notices.push(notice);
    }

    /// The batch to send, leaving this one empty.
    pub(super) fn take(&mut self) -> WorkspaceNotifyBatch {
        self.index.clear();
        let mut notices = std::mem::take(&mut self.notices);
        notices.append(&mut self.resyncs);
        WorkspaceNotifyBatch { notices }
    }

    fn resyncing(&self, root_id: &str) -> bool {
        self.resyncs.iter().any(|resync| resync.root_id == root_id)
    }

    /// Replaces the root's queued paths with one resync notice.
    fn resync(&mut self, mut notice: WorkspaceNotifyNotice) {
        notice.relative_path = OVERFLOW_RELATIVE_PATH.to_string();
        notice.kind = RESYNC_KIND.to_string();
        self.notices
            .retain(|queued| queued.root_id != notice.root_id);
        self.index = self
            .notices
            .iter()
            .enumerate()
            .map(|(slot, queued)| ((queued.root_id.clone(), queued.relative_path.clone()), slot))
            .collect();
        self.resyncs.push(notice);
    }
}

/// A watch's batch, shared by its watcher threads and its emitter.
#[derive(Default)]
pub(super) struct NoticeSink {
    batch: Mutex<NoticeBatch>,
    queued: Condvar,
}

impl NoticeSink {
    pub(super) fn push(&self, notice: WorkspaceNotifyNotice) {
        self.lock().push(notice);
        self.queued.notify_one();
    }

    fn lock(&self) -> MutexGuard<'_, NoticeBatch> {
        self.batch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sends a batch for each window that held notices, until `cancelled`.
    pub(super) fn run_emitter(
        &self,
        cancelled: &AtomicBool,
        mut emit: impl FnMut(WorkspaceNotifyBatch),
    ) {
        loop {
            let mut batch = self.lock();
            while batch.is_empty() {
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                batch = self
                    .queued
                    .wait_timeout(batch, CANCEL_CHECK)
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .0;
            }
            drop(batch);
            std::thread::sleep(NOTICE_BATCH_WINDOW);
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            let batch = self.lock().take();
            emit(batch);
        }
    }
}

/// Starts the emitter of one watch, which ends when the watch is cancelled.
pub(super) fn spawn_notice_emitter(
    sink: Arc<NoticeSink>,
    cancelled: Arc<AtomicBool>,
    emit: impl FnMut(WorkspaceNotifyBatch) + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("sage-workspace-notify-emit".into())
        .spawn(move || sink.run_emitter(&cancelled, emit))
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn notice(root: &str, path: &str, kind: &str) -> WorkspaceNotifyNotice {
        WorkspaceNotifyNotice {
            project_id: "project-a".into(),
            project_generation: 3,
            transition_epoch: 7,
            root_id: root.into(),
            relative_path: path.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn repeated_paths_coalesce_to_their_latest_kind() {
        let mut batch = NoticeBatch::default();
        batch.push(notice("root-a", "src/main.rs", "created"));
        batch.push(notice("root-a", "src/lib.rs", "changed"));
        batch.push(notice("root-a", "src/main.rs", "changed"));
        batch.push(notice("root-b", "src/main.rs", "deleted"));
        assert_eq!(
            batch.take().notices,
            vec![
                notice("root-a", "src/main.rs", "changed"),
                notice("root-a", "src/lib.rs", "changed"),
                notice("root-b", "src/main.rs", "deleted"),
            ]
        );
        assert!(batch.is_empty());
    }

    #[test]
    fn a_burst_past_the_bound_becomes_one_resync_for_that_root() {
        let mut batch = NoticeBatch::default();
        batch.push(notice("root-b", "README.md", "changed"));
        for index in 0..10_000 {
            batch.push(notice("root-a", &format!("gen/file-{index}.rs"), "created"));
        }
        let sent = batch.take().notices;
        assert_eq!(
            sent,
            vec![
                notice("root-b", "README.md", "changed"),
                notice("root-a", OVERFLOW_RELATIVE_PATH, RESYNC_KIND),
            ]
        );
    }

    #[test]
    fn a_lost_queue_replaces_the_roots_paths_with_one_resync() {
        let mut batch = NoticeBatch::default();
        batch.push(notice("root-a", "src/main.rs", "changed"));
        batch.push(notice("root-a", OVERFLOW_RELATIVE_PATH, RESYNC_KIND));
        batch.push(notice("root-a", "src/lib.rs", "changed"));
        batch.push(notice("root-a", OVERFLOW_RELATIVE_PATH, RESYNC_KIND));
        assert_eq!(
            batch.take().notices,
            vec![notice("root-a", OVERFLOW_RELATIVE_PATH, RESYNC_KIND)]
        );
    }

    #[test]
    fn a_burst_reaches_the_webview_as_one_bounded_event() {
        let sink = Arc::new(NoticeSink::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (sent, received) = mpsc::channel();
        spawn_notice_emitter(Arc::clone(&sink), Arc::clone(&cancelled), move |batch| {
            let _ = sent.send(batch);
        })
        .expect("start emitter");

        for index in 0..1_000 {
            sink.push(notice(
                "root-a",
                &format!("src/file-{}.rs", index % 50),
                "changed",
            ));
        }
        let first = received
            .recv_timeout(Duration::from_secs(2))
            .expect("one event for the burst");
        assert_eq!(first.notices.len(), 50);
        assert!(
            received.recv_timeout(NOTICE_BATCH_WINDOW * 3).is_err(),
            "the burst should not send a second event"
        );

        cancelled.store(true, Ordering::Release);
        sink.push(notice("root-a", "src/late.rs", "changed"));
        assert!(received.recv_timeout(NOTICE_BATCH_WINDOW * 3).is_err());
    }
}
