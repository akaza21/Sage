//! Linux root watches against real inotify: each test changes a temporary
//! Project and checks the exact paths reported, with bounded waits only.

use super::{linux, run_prepared, unix_events};
use crate::workspace_notify::{OVERFLOW_NOTIFY_ACTION, OVERFLOW_RELATIVE_PATH};
use std::{
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Notice = (String, u32);

fn test_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sage-linux-notify-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos(),
    ));
    fs::create_dir(&root).expect("create test root");
    root.canonicalize().expect("canonical test root")
}

fn overflow() -> Notice {
    (OVERFLOW_RELATIVE_PATH.to_string(), OVERFLOW_NOTIFY_ACTION)
}

fn created(relative: &str) -> Notice {
    (relative.to_string(), 1)
}

/// A root watch set up as the Project watch sets it up.
struct Watch {
    cancelled: Arc<AtomicBool>,
    notices: mpsc::Receiver<Notice>,
    ended: mpsc::Receiver<std::io::Result<()>>,
}

impl Watch {
    fn start(root: &Path) -> Self {
        Self::start_within(root, unix_events::MAX_UNIX_WATCH_ENTRIES)
    }

    /// A root watch whose walks examine at most `max_entries` entries.
    fn start_within(root: &Path, max_entries: u64) -> Self {
        let (directories, truncated) =
            unix_events::bounded_watch_directories(root).expect("walk test root");
        assert!(!truncated, "test roots stay within the watch cap");
        let prepared =
            linux::prepare_within(&directories, true, max_entries).expect("watch test root");
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, notices) = mpsc::sync_channel(256);
        let (finished, ended) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = run_prepared(prepared, &worker_cancelled, &mut |relative, action| {
                let _ = sender.try_send((relative, action));
            });
            let _ = finished.send(result);
        });
        Self {
            cancelled,
            notices,
            ended,
        }
    }

    /// How the watch ended, if it ended on its own within two seconds.
    fn ended(&self) -> Option<std::io::Result<()>> {
        self.ended.recv_timeout(Duration::from_secs(2)).ok()
    }

    /// The notices received until `expected` arrives or two seconds pass.
    fn until(&self, expected: &Notice) -> Vec<Notice> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut received = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Ok(notice) = self.notices.recv_timeout(left) else {
                break;
            };
            let done = &notice == expected;
            received.push(notice);
            if done {
                break;
            }
        }
        received
    }

    fn stop(self) {
        self.cancelled.store(true, Ordering::Release);
        self.ended
            .recv_timeout(Duration::from_secs(2))
            .expect("cancelled watch should stop promptly")
            .expect("cancelled watch should not fail");
    }
}

/// Asserts that `received` ends with `expected` and holds nothing else but
/// resyncs, which a tree change may repeat when its records span two reads.
fn assert_only(received: &[Notice], expected: &Notice) {
    assert_eq!(received.last(), Some(expected), "got {received:?}");
    assert!(
        received
            .iter()
            .all(|notice| notice == expected || notice == &overflow()),
        "got {received:?}"
    );
}

#[test]
fn subfolder_change_is_reported_at_its_real_path() {
    let root = test_root("subfolder");
    fs::create_dir(root.join("src")).expect("create src");
    let watch = Watch::start(&root);
    fs::write(root.join("src/main.ts"), "created").expect("write src/main.ts");
    let received = watch.until(&created("src/main.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(received, vec![created("src/main.ts")]);
}

#[test]
fn non_utf8_subfolder_is_watched_and_resyncs() {
    let root = test_root("non-utf8");
    let folder = root.join(std::ffi::OsStr::from_bytes(b"bad\xff"));
    fs::create_dir(&folder).expect("create non-UTF-8 folder");
    let watch = Watch::start(&root);
    fs::write(folder.join("main.ts"), "created").expect("write in non-UTF-8 folder");
    let received = watch.until(&overflow());
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(received, vec![overflow()]);
}

#[test]
fn renamed_subfolder_reports_its_new_path() {
    let root = test_root("rename");
    fs::create_dir(root.join("src")).expect("create src");
    let watch = Watch::start(&root);
    fs::rename(root.join("src"), root.join("lib")).expect("rename src to lib");
    let renamed = watch.until(&overflow());
    fs::write(root.join("lib/main.ts"), "created").expect("write lib/main.ts");
    let written = watch.until(&created("lib/main.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(renamed, vec![overflow()]);
    assert_only(&written, &created("lib/main.ts"));
}

#[test]
fn subfolder_moved_out_of_the_root_stops_reporting() {
    let root = test_root("move-out");
    let outside = test_root("move-out-target");
    fs::create_dir(root.join("src")).expect("create src");
    let watch = Watch::start(&root);
    fs::rename(root.join("src"), outside.join("src")).expect("move src out");
    let moved = watch.until(&overflow());
    fs::write(outside.join("src/main.ts"), "outside").expect("write outside");
    fs::write(root.join("marker.ts"), "created").expect("write marker");
    let written = watch.until(&created("marker.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    fs::remove_dir_all(&outside).expect("remove move target");
    assert_eq!(moved, vec![overflow()]);
    assert_only(&written, &created("marker.ts"));
}

#[test]
fn folder_created_after_start_is_watched() {
    let root = test_root("new-folder");
    let watch = Watch::start(&root);
    fs::create_dir(root.join("new")).expect("create new");
    let made = watch.until(&overflow());
    fs::write(root.join("new/main.ts"), "created").expect("write new/main.ts");
    let written = watch.until(&created("new/main.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(made, vec![overflow()]);
    assert_only(&written, &created("new/main.ts"));
}

#[test]
fn deleted_and_recreated_subfolder_is_watched_again() {
    let root = test_root("recreate");
    fs::create_dir(root.join("src")).expect("create src");
    let watch = Watch::start(&root);
    fs::remove_dir(root.join("src")).expect("delete src");
    let deleted = watch.until(&overflow());
    fs::create_dir(root.join("src")).expect("recreate src");
    let recreated = watch.until(&overflow());
    fs::write(root.join("src/main.ts"), "created").expect("write src/main.ts");
    let written = watch.until(&created("src/main.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(deleted, vec![overflow()]);
    assert_eq!(recreated, vec![overflow()]);
    assert_only(&written, &created("src/main.ts"));
}

#[test]
fn resync_rearms_before_a_client_recreates_then_writes() {
    let root = test_root("resync-order");
    fs::create_dir(root.join("src")).expect("create src");
    let prepared = linux::prepare(std::slice::from_ref(&root), true).expect("prepare tree watch");
    // Queue both parent and self attribute events and the entire deletion
    // before reading. Neither attribute event may acknowledge a tree resync.
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(root.join("src"), fs::Permissions::from_mode(0o700))
        .expect("change src permissions");
    fs::remove_dir(root.join("src")).expect("delete src");
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let worker_root = root.clone();
    let (sender, notices) = mpsc::sync_channel(256);
    let (finished, ended) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let mut resyncs = 0;
        let result = run_prepared(prepared, &worker_cancelled, &mut |relative, action| {
            if action == OVERFLOW_NOTIFY_ACTION {
                resyncs += 1;
                match resyncs {
                    1 => fs::create_dir(worker_root.join("src")).expect("client recreates src"),
                    2 => fs::write(worker_root.join("src/main.ts"), "created")
                        .expect("client writes immediately after resync"),
                    _ => {}
                }
            }
            let _ = sender.try_send((relative, action));
        });
        let _ = finished.send(result);
    });
    let watch = Watch {
        cancelled,
        notices,
        ended,
    };
    let written = watch.until(&created("src/main.ts"));
    watch.stop();
    worker.join().expect("watch worker should not panic");
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(
        written,
        vec![
            ("src".to_string(), 3),
            ("src".to_string(), 3),
            overflow(),
            overflow(),
            created("src/main.ts")
        ]
    );
}

#[test]
fn folder_past_the_watch_cap_resyncs_and_the_watch_keeps_running() {
    let root = test_root("cap");
    for index in 1..unix_events::MAX_UNIX_WATCH_DIRECTORIES {
        fs::create_dir(root.join(format!("dir-{index}"))).expect("create folder");
    }
    let watch = Watch::start(&root);
    fs::create_dir(root.join("extra")).expect("create folder past the cap");
    let made = watch.until(&overflow());
    fs::write(root.join("marker.ts"), "created").expect("write marker");
    let written = watch.until(&created("marker.ts"));
    watch.stop();
    let (directories, truncated) =
        unix_events::bounded_watch_directories(&root).expect("walk test root");
    fs::remove_dir_all(&root).expect("remove test root");
    assert!(truncated);
    assert_eq!(directories.len(), unix_events::MAX_UNIX_WATCH_DIRECTORIES);
    assert_eq!(made, vec![overflow()]);
    assert_only(&written, &created("marker.ts"));
}

/// Swaps two paths in one step, as `renameat2` with `RENAME_EXCHANGE`.
fn exchange(first: &Path, second: &Path) {
    let first = std::ffi::CString::new(first.as_os_str().as_bytes()).expect("first path");
    let second = std::ffi::CString::new(second.as_os_str().as_bytes()).expect("second path");
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            first.as_ptr(),
            libc::AT_FDCWD,
            second.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    assert_eq!(result, 0, "exchange: {}", std::io::Error::last_os_error());
}

#[test]
fn root_swapped_for_a_symlink_stops_the_watch() {
    let root = test_root("root-swap");
    let outside = test_root("root-swap-outside");
    fs::create_dir(root.join("src")).expect("create src");
    let link = root.with_extension("link");
    std::os::unix::fs::symlink(&outside, &link).expect("link to outside");
    let watch = Watch::start(&root);
    exchange(&root, &link);
    fs::write(outside.join("main.ts"), "outside").expect("write outside");
    let ended = watch.ended();
    let received = watch.notices.try_iter().collect::<Vec<_>>();
    fs::remove_file(&root).expect("remove link at the root's path");
    fs::remove_dir_all(&link).expect("remove moved root");
    fs::remove_dir_all(&outside).expect("remove outside");
    assert!(matches!(ended, Some(Err(_))), "got {ended:?}");
    assert_eq!(received, Vec::new());
}

#[test]
fn subfolder_swapped_for_a_symlink_is_not_watched_through_it() {
    let root = test_root("link-swap");
    let outside = test_root("link-swap-outside");
    fs::create_dir(root.join("src")).expect("create src");
    let link = outside.with_extension("link");
    std::os::unix::fs::symlink(&outside, &link).expect("link to outside");
    let watch = Watch::start(&root);
    exchange(&root.join("src"), &link);
    let swapped = watch.until(&overflow());
    fs::write(outside.join("main.ts"), "outside").expect("write outside");
    fs::write(root.join("marker.ts"), "created").expect("write marker");
    let written = watch.until(&created("marker.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    fs::remove_dir_all(&link).expect("remove moved src");
    fs::remove_dir_all(&outside).expect("remove outside");
    assert_eq!(swapped.last(), Some(&overflow()), "got {swapped:?}");
    assert_eq!(
        written.last(),
        Some(&created("marker.ts")),
        "got {written:?}"
    );
    assert!(
        swapped
            .iter()
            .chain(&written)
            .all(|(relative, _)| !relative.starts_with("src/")),
        "got {swapped:?} then {written:?}"
    );
}

#[test]
fn rewatch_past_the_entry_limit_leaves_the_rest_unwatched() {
    let root = test_root("entries");
    fs::create_dir_all(root.join("a/b")).expect("create a/b");
    let watch = Watch::start_within(&root, 2);
    fs::create_dir(root.join("a/b/c")).expect("create a/b/c");
    let made = watch.until(&overflow());
    fs::write(root.join("a/b/c/main.ts"), "created").expect("write a/b/c/main.ts");
    fs::write(root.join("marker.ts"), "created").expect("write marker");
    let written = watch.until(&created("marker.ts"));
    watch.stop();
    fs::remove_dir_all(&root).expect("remove test root");
    assert_eq!(made, vec![overflow()]);
    assert_only(&written, &created("marker.ts"));
}
