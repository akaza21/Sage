//! Coalesces a watch's notices into one WebView event per short window.
//!
//! Watcher threads push accepted notices; one emitter thread per watch
//! sends them as a single batch once the window that the first notice opened
//! has passed. Within a batch a path appears once per meaning (its own
//! change, or a change inside it when it is an ignored folder), with its
//! latest kind. A batch never holds more than [`MAX_BATCH_NOTICES`] entries,
//! resyncs included: when one more would pass that, the root holding the
//! most paths is sent as one resync notice in their place, as a lost
//! watcher queue is, and the WebView reloads that root's listings.

use std::{
    collections::HashMap,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Serialize;

use super::git_relevance::{GitRefresh, GitWindow};
use super::{MAX_ROOTS, OVERFLOW_RELATIVE_PATH, WorkspaceNotifyNotice};

/// How long the first notice of a batch waits for others. The WebView's Git
/// refresh settles for 300 ms anyway, so 100 ms adds no visible delay while
/// capping a build's writes at ten events a second.
pub(super) const NOTICE_BATCH_WINDOW: Duration = Duration::from_millis(100);
/// The most notices one event carries; the WebView refuses a larger event
/// (`MAX_NOTIFY_BATCH_NOTICES` in `workspaceNotifyCompose.ts`, which must stay
/// equal). A folder refresh per notice is the WebView's cost, and a root
/// with more changes than this in 100 ms is cheaper to resync once.
pub(super) const MAX_BATCH_NOTICES: usize = 256;
/// How often an idle emitter checks whether its watch was cancelled, which
/// matches the watcher threads' own wait.
const CANCEL_CHECK: Duration = Duration::from_millis(100);

// A full batch spreads its paths over at most MAX_ROOTS roots, so its
// largest root holds several and resyncing it always makes room.
const _: () = assert!(MAX_ROOTS * 2 <= MAX_BATCH_NOTICES);

const RESYNC_KIND: &str = "resync";
const IGNORED_KIND: &str = "ignored";

/// The payload of one WebView event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceNotifyBatch {
    pub notices: Vec<WorkspaceNotifyNotice>,
}

/// A queued path: its root, its path, and whether it stands for changes
/// inside an ignored folder rather than for the path itself.
type PathKey = (String, String, bool);

fn path_key(notice: &WorkspaceNotifyNotice) -> PathKey {
    (
        notice.root_id.clone(),
        notice.relative_path.clone(),
        notice.kind == IGNORED_KIND,
    )
}

/// Notices gathered since the last event.
#[derive(Debug, Default)]
pub(super) struct NoticeBatch {
    git_windows: HashMap<String, GitWindow>,
    notices: Vec<WorkspaceNotifyNotice>,
    /// Each queued notice's index.
    index: HashMap<PathKey, usize>,
    /// The resync notice of each root that overflowed or lost records. Its
    /// paths are dropped, since the resync reloads every listing they touch.
    resyncs: Vec<WorkspaceNotifyNotice>,
}

impl NoticeBatch {
    pub(super) fn push_record(
        &mut self,
        notice: WorkspaceNotifyNotice,
        original: &str,
        directory: bool,
        moved: bool,
    ) {
        self.git_windows
            .entry(notice.root_id.clone())
            .or_default()
            .push(original, directory, moved);
        self.push(notice);
    }

    pub(super) fn take_git_windows(&mut self) -> HashMap<String, GitWindow> {
        std::mem::take(&mut self.git_windows)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.notices.is_empty() && self.resyncs.is_empty()
    }

    fn len(&self) -> usize {
        self.notices.len() + self.resyncs.len()
    }

    pub(super) fn push(&mut self, notice: WorkspaceNotifyNotice) {
        if self.resyncing(&notice.root_id) {
            return;
        }
        if notice.kind == RESYNC_KIND {
            self.resync(notice);
        } else {
            let key = path_key(&notice);
            if let Some(&slot) = self.index.get(&key) {
                self.notices[slot].kind = notice.kind;
                return;
            }
            self.index.insert(key, self.notices.len());
            self.notices.push(notice);
        }
        while self.len() > MAX_BATCH_NOTICES {
            let Some(fullest) = self.fullest_root() else {
                break;
            };
            let template = self
                .notices
                .iter()
                .find(|queued| queued.root_id == fullest)
                .cloned()
                .expect("the fullest root has a queued path");
            self.resync(template);
        }
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

    /// The root with the most queued paths.
    fn fullest_root(&self) -> Option<String> {
        let mut counts = HashMap::<&str, usize>::new();
        for queued in &self.notices {
            *counts.entry(queued.root_id.as_str()).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by(|left, right| left.1.cmp(&right.1).then(right.0.cmp(left.0)))
            .map(|(root, _)| root.to_string())
    }

    /// Replaces the root's queued paths with one resync notice.
    fn resync(&mut self, mut notice: WorkspaceNotifyNotice) {
        notice.relative_path = OVERFLOW_RELATIVE_PATH.to_string();
        notice.kind = RESYNC_KIND.to_string();
        notice.git_relevant = None;
        self.notices
            .retain(|queued| queued.root_id != notice.root_id);
        self.index = self
            .notices
            .iter()
            .enumerate()
            .map(|(slot, queued)| (path_key(queued), slot))
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
    pub(super) fn push_record(
        &self,
        notice: WorkspaceNotifyNotice,
        original: &str,
        directory: bool,
        moved: bool,
    ) {
        self.lock().push_record(notice, original, directory, moved);
        self.queued.notify_one();
    }

    #[cfg(test)]
    fn push(&self, notice: WorkspaceNotifyNotice) {
        self.lock().push(notice);
        self.queued.notify_one();
    }

    fn lock(&self) -> MutexGuard<'_, NoticeBatch> {
        self.batch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sends one batch per window that held notices, until `cancelled`. A
    /// window opens with its first notice and lasts as long as `window`
    /// blocks.
    fn run_emitter(
        &self,
        cancelled: &AtomicBool,
        mut window: impl FnMut(),
        mut prepare: impl FnMut(&mut WorkspaceNotifyBatch, HashMap<String, GitWindow>),
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
            window();
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            let (mut batch, windows) = {
                let mut pending = self.lock();
                let windows = pending.take_git_windows();
                (pending.take(), windows)
            };
            prepare(&mut batch, windows);
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            emit(batch);
        }
    }
}

/// Starts the emitter of one watch, which ends when the watch is cancelled.
pub(super) fn spawn_notice_emitter(
    sink: Arc<NoticeSink>,
    cancelled: Arc<AtomicBool>,
    roots: Vec<(String, std::path::PathBuf)>,
    emit: impl FnMut(WorkspaceNotifyBatch) + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("sage-workspace-notify-emit".into())
        .spawn(move || {
            let mut git = GitRefresh::new(roots);
            sink.run_emitter(
                &cancelled,
                || std::thread::sleep(NOTICE_BATCH_WINDOW),
                |batch, windows| git.apply(batch, windows, &cancelled),
                emit,
            )
        })
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
            git_relevant: None,
        }
    }

    fn resync(root: &str) -> WorkspaceNotifyNotice {
        notice(root, OVERFLOW_RELATIVE_PATH, RESYNC_KIND)
    }

    #[test]
    fn repeated_paths_coalesce_to_their_latest_kind() {
        let mut batch = NoticeBatch::default();
        batch.push(notice("root-a", "src/main.rs", "created"));
        batch.push(notice("root-a", "src/lib.rs", "changed"));
        batch.push(notice("root-a", "src/main.rs", "changed"));
        batch.push(notice("root-b", "src/main.rs", "deleted"));
        batch.push(notice("root-a", "target", "created"));
        batch.push(notice("root-a", "target", IGNORED_KIND));
        batch.push(notice("root-a", "target", IGNORED_KIND));
        assert_eq!(
            batch.take().notices,
            vec![
                notice("root-a", "src/main.rs", "changed"),
                notice("root-a", "src/lib.rs", "changed"),
                notice("root-b", "src/main.rs", "deleted"),
                notice("root-a", "target", "created"),
                notice("root-a", "target", IGNORED_KIND),
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
        assert_eq!(
            batch.take().notices,
            vec![notice("root-b", "README.md", "changed"), resync("root-a")]
        );
    }

    #[test]
    fn two_roots_at_the_bound_stay_within_it() {
        let mut batch = NoticeBatch::default();
        let half = MAX_BATCH_NOTICES / 2;
        for index in 0..half {
            batch.push(notice("root-a", &format!("a-{index}"), "changed"));
            batch.push(notice("root-b", &format!("b-{index}"), "changed"));
        }
        batch.push(notice("root-c", "c-0", "changed"));
        batch.push(resync("root-d"));
        for index in 0..MAX_BATCH_NOTICES {
            batch.push(notice("root-b", &format!("b-more-{index}"), "changed"));
        }
        let sent = batch.take().notices;
        assert!(sent.len() <= MAX_BATCH_NOTICES, "{} notices", sent.len());
        let resyncs = sent
            .iter()
            .filter(|sent| sent.kind == RESYNC_KIND)
            .map(|sent| sent.root_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(resyncs, ["root-a", "root-d", "root-b"]);
        assert!(sent.contains(&notice("root-c", "c-0", "changed")));
        assert!(
            sent.iter()
                .all(|sent| sent.kind == RESYNC_KIND || sent.root_id == "root-c")
        );
    }

    #[test]
    fn a_lost_queue_replaces_the_roots_paths_with_one_resync() {
        let mut batch = NoticeBatch::default();
        batch.push(notice("root-a", "src/main.rs", "changed"));
        batch.push(resync("root-a"));
        batch.push(notice("root-a", "src/lib.rs", "changed"));
        batch.push(resync("root-a"));
        assert_eq!(batch.take().notices, vec![resync("root-a")]);
    }

    #[test]
    fn a_burst_within_one_window_reaches_the_webview_as_one_event() {
        let sink = Arc::new(NoticeSink::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (window_open, window_opened) = mpsc::channel();
        let (close_window, window_closes) = mpsc::channel::<()>();
        let (sent, received) = mpsc::channel();
        let emitter = {
            let sink = Arc::clone(&sink);
            let cancelled = Arc::clone(&cancelled);
            std::thread::spawn(move || {
                sink.run_emitter(
                    &cancelled,
                    || {
                        window_open.send(()).unwrap();
                        let _ = window_closes.recv();
                    },
                    |_, _| {},
                    |batch| sent.send(batch).unwrap(),
                )
            })
        };

        sink.push(notice("root-a", "src/file-0.rs", "changed"));
        window_opened
            .recv()
            .expect("the first notice opens a window");
        for index in 0..1_000 {
            sink.push(notice(
                "root-a",
                &format!("src/file-{}.rs", index % 50),
                "changed",
            ));
        }
        close_window.send(()).unwrap();
        let first = received.recv().expect("one event for the window");
        assert_eq!(first.notices.len(), 50);

        sink.push(notice("root-a", "src/late.rs", "changed"));
        window_opened
            .recv()
            .expect("a later notice opens a new window");
        close_window.send(()).unwrap();
        assert_eq!(
            received
                .recv()
                .expect("one event for the next window")
                .notices,
            vec![notice("root-a", "src/late.rs", "changed")]
        );

        cancelled.store(true, Ordering::Release);
        emitter.join().expect("the emitter stops once cancelled");
        assert!(received.try_recv().is_err());
    }
}
