use super::*;
use crate::editor_watch::EditorExternalWatchKind;
use std::{
    fs,
    io::Write,
    sync::mpsc::{self, Receiver},
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const WAIT: Duration = Duration::from_secs(2);

struct WatchTest {
    directory: PathBuf,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
    notices: Receiver<EditorExternalWatchKind>,
    completed: Receiver<std::io::Result<()>>,
    worker: Option<JoinHandle<()>>,
}

impl WatchTest {
    fn start() -> Self {
        Self::start_with_target(true)
    }

    fn start_with_target(present: bool) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sage-editor-watch-unix-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let path = directory.join("notes.md");
        fs::write(&path, "initial").unwrap();
        if !present {
            fs::remove_file(&path).unwrap();
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        // Same prepare-before-spawn seam as production: target exists before registration.
        let prepared = prepare_editor_watch(&path).expect("prepare editor watch");
        let (notices, receive_notice) = mpsc::sync_channel(16);
        let (completed, receive_completion) = mpsc::sync_channel(1);
        let (ready, receive_ready) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let result = run_prepared(prepared, &worker_cancelled, &mut |relative, action| {
                if let Some(kind) =
                    editor_notice_kind_from_unix_delivery("notes.md", &relative, action)
                {
                    // A full test mailbox must never prevent cancellation or cleanup.
                    let _ = notices.try_send(kind);
                }
            });
            completed.send(result).unwrap();
        });
        let test = Self {
            directory,
            path,
            cancelled,
            notices: receive_notice,
            completed: receive_completion,
            worker: Some(worker),
        };
        receive_ready
            .recv_timeout(WAIT)
            .expect("worker ready after registration");
        test
    }

    fn write_in_place(&self) {
        let mut file = fs::OpenOptions::new().write(true).open(&self.path).unwrap();
        file.write_all(b"changed").unwrap();
        file.sync_all().unwrap();
    }

    fn expect(&self, kind: EditorExternalWatchKind) {
        assert_eq!(self.notices.recv_timeout(WAIT).expect("watch notice"), kind);
    }

    #[cfg(target_os = "macos")]
    fn settle(&self) {
        // Drain already-coalesced parent/target notices before testing the new vnode.
        // This never retries a mutation, and a busy watcher fails rather than waiting forever.
        for _ in 0..16 {
            if self
                .notices
                .recv_timeout(Duration::from_millis(150))
                .is_err()
            {
                return;
            }
        }
        panic!("watch did not settle within the bounded event budget");
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.completed
            .recv_timeout(WAIT)
            .expect("prompt cancellation")
            .expect("clean stop");
        self.worker.take().unwrap().join().unwrap();
    }
}

impl Drop for WatchTest {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn unix_editor_watch_existing_target_in_place_write_then_cancels() {
    let mut test = WatchTest::start();
    test.write_in_place();
    test.expect(EditorExternalWatchKind::Changed);
    test.cancel();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_idle_cancellation() {
    let mut test = WatchTest::start();
    test.cancel();
    assert!(test.notices.try_recv().is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_delete_recreate_and_write_reopens_target() {
    let mut test = WatchTest::start();
    fs::remove_file(&test.path).unwrap();
    test.expect(EditorExternalWatchKind::Deleted);
    fs::write(test.directory.join("sibling.md"), "unrelated").unwrap();
    assert!(
        test.notices
            .recv_timeout(Duration::from_millis(250))
            .is_err()
    );
    fs::write(&test.path, "initial").unwrap();
    test.expect(EditorExternalWatchKind::Changed);
    test.settle();
    test.write_in_place();
    test.expect(EditorExternalWatchKind::Changed);
    test.cancel();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_atomic_replacement_and_write_reopens_target() {
    let mut test = WatchTest::start();
    let replacement = test.directory.join("replacement.md");
    fs::write(&replacement, "replace").unwrap();
    test.settle();
    fs::rename(replacement, &test.path).unwrap();
    test.expect(EditorExternalWatchKind::Changed);
    test.settle();
    test.write_in_place();
    test.expect(EditorExternalWatchKind::Changed);
    test.cancel();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_target_rename_reports_deleted_then_reopens() {
    let mut test = WatchTest::start();
    let moved = test.directory.join("moved.md");
    fs::rename(&test.path, &moved).unwrap();
    test.expect(EditorExternalWatchKind::Deleted);
    test.settle();
    fs::rename(moved, &test.path).unwrap();
    test.expect(EditorExternalWatchKind::Changed);
    test.settle();
    test.write_in_place();
    test.expect(EditorExternalWatchKind::Changed);
    test.cancel();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_parent_rename_ends_with_failure_resync() {
    let mut test = WatchTest::start();
    let moved = test.directory.with_extension("moved");
    fs::rename(&test.directory, &moved).unwrap();
    test.directory = moved;
    let result = test
        .completed
        .recv_timeout(WAIT)
        .expect("invalid parent ends worker");
    assert!(result.is_err());
    assert_eq!(
        crate::editor_watch::editor_watch_failure_resync(false),
        Some(EditorExternalWatchKind::Changed)
    );
    test.worker.take().unwrap().join().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_rejects_symlink_replacement() {
    let mut test = WatchTest::start();
    let replacement = test.directory.join("replacement.md");
    std::os::unix::fs::symlink(&test.path, &replacement).unwrap();
    test.settle();
    fs::rename(replacement, &test.path).unwrap();
    assert!(
        test.completed
            .recv_timeout(WAIT)
            .expect("unsafe replacement ends worker")
            .is_err()
    );
    test.worker.take().unwrap().join().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_missing_during_prepare_reports_once_then_recovers() {
    let mut test = WatchTest::start_with_target(false);
    test.expect(EditorExternalWatchKind::Deleted);
    fs::write(test.directory.join("sibling.md"), "unrelated").unwrap();
    assert!(
        test.notices
            .recv_timeout(Duration::from_millis(250))
            .is_err()
    );
    fs::write(&test.path, "initial").unwrap();
    test.expect(EditorExternalWatchKind::Changed);
    test.settle();
    test.write_in_place();
    test.expect(EditorExternalWatchKind::Changed);
    test.cancel();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_parent_deletion_ends_worker() {
    let mut test = WatchTest::start();
    fs::remove_file(&test.path).unwrap();
    test.expect(EditorExternalWatchKind::Deleted);
    test.settle();
    fs::remove_dir(&test.directory).unwrap();
    assert!(
        test.completed
            .recv_timeout(WAIT)
            .expect("deleted parent ends worker")
            .is_err()
    );
    test.worker.take().unwrap().join().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_editor_watch_precancelled_queue_does_not_deliver() {
    let mut test = WatchTest::start();
    test.cancel();
    let prepared = prepare_editor_watch(&test.path).unwrap();
    test.write_in_place();
    run_prepared(prepared, &test.cancelled, &mut |_, _| {
        panic!("cancelled watcher must not deliver queued writes");
    })
    .unwrap();
}
