use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tauri::{AppHandle, Emitter};

use super::{
    WORKSPACE_NOTIFY_EVENT, WorkspaceNotifyError, apply_unwatch_if_generation,
    deliver_workspace_notify_action, reject, unix_events,
};

pub(super) fn spawn_workspace_root_watch(
    app: AppHandle,
    project_id: String,
    generation: u32,
    root_id: String,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    let (directories, truncated) = unix_events::bounded_watch_directories(&path)?;
    let prepared = prepare_watch(&directories).map_err(|_| reject("notify-substituted"))?;
    std::thread::Builder::new()
        .name("sage-workspace-notify".into())
        .spawn(move || {
            let mut deliver = |relative: String, action: u32| {
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                if let Ok(Some(notice)) = deliver_workspace_notify_action(
                    &project_id,
                    generation,
                    &root_id,
                    &relative,
                    action,
                ) {
                    let _ = app.emit(WORKSPACE_NOTIFY_EVENT, notice);
                }
            };
            if truncated
                && let Ok((relative, action)) =
                    unix_events::unix_watch_delivery(None, unix_events::UnixWatchKind::Overflow)
            {
                deliver(relative, action);
            }
            let result = run_prepared(prepared, &cancelled, &mut deliver);
            if result.is_err() && !cancelled.load(Ordering::Acquire) {
                let _ = apply_unwatch_if_generation(&project_id, generation);
            }
        })
        .map(|_| ())
        .map_err(|_| reject("notify-substituted"))
}

#[cfg(target_os = "macos")]
pub(crate) fn prepare_watch(directories: &[PathBuf]) -> std::io::Result<macos::Prepared> {
    macos::prepare(directories)
}

#[cfg(target_os = "linux")]
pub(crate) fn prepare_watch(directories: &[PathBuf]) -> std::io::Result<linux::Prepared> {
    linux::prepare(directories)
}

#[cfg(target_os = "macos")]
pub(crate) fn run_prepared(
    prepared: macos::Prepared,
    cancelled: &AtomicBool,
    deliver: &mut impl FnMut(String, u32),
) -> std::io::Result<()> {
    macos::run(prepared, cancelled, deliver)
}

#[cfg(target_os = "linux")]
pub(crate) fn run_prepared(
    prepared: linux::Prepared,
    cancelled: &AtomicBool,
    deliver: &mut impl FnMut(String, u32),
) -> std::io::Result<()> {
    linux::run(prepared, cancelled, deliver)
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) fn prepare_editor_watch(path: &std::path::Path) -> std::io::Result<macos::Prepared> {
    macos::prepare_editor(path)
}

#[cfg(target_os = "linux")]
pub(crate) fn prepare_editor_watch(path: &std::path::Path) -> std::io::Result<linux::Prepared> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "editor watch parent")
    })?;
    prepare_watch(&[parent.to_path_buf()])
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{AtomicBool, Ordering, unix_events};
    use std::{
        ffi::CString,
        os::{
            raw::{c_char, c_int},
            unix::ffi::OsStrExt,
        },
    };

    const IN_CLOEXEC: c_int = 0x8_0000;
    const IN_NONBLOCK: c_int = 0x800;
    const IN_WATCH_MASK: u32 = unix_events::IN_MODIFY
        | unix_events::IN_ATTRIB
        | unix_events::IN_MOVED_FROM
        | unix_events::IN_MOVED_TO
        | unix_events::IN_CREATE
        | unix_events::IN_DELETE
        | unix_events::IN_Q_OVERFLOW;
    const POLLIN: i16 = 0x001;

    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn inotify_init1(flags: c_int) -> c_int;
        fn inotify_add_watch(fd: c_int, pathname: *const c_char, mask: u32) -> c_int;
        fn close(fd: c_int) -> c_int;
        fn read(fd: c_int, buf: *mut u8, count: usize) -> isize;
        fn poll(fds: *mut PollFd, nfds: u64, timeout: c_int) -> c_int;
    }

    pub(crate) struct Prepared {
        fd: c_int,
        folders: unix_events::InotifyFolders,
    }

    impl Drop for Prepared {
        fn drop(&mut self) {
            unsafe { close(self.fd) };
        }
    }

    pub(super) fn prepare(directories: &[std::path::PathBuf]) -> std::io::Result<Prepared> {
        let fd = unsafe { inotify_init1(IN_CLOEXEC | IN_NONBLOCK) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Dropping `prepared` on any early return closes the descriptor.
        let mut prepared = Prepared {
            fd,
            folders: unix_events::InotifyFolders::new(),
        };
        let first = directories.first();
        for directory in directories {
            let path = CString::new(directory.as_os_str().as_bytes()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "notify path")
            })?;
            let watch = unsafe { inotify_add_watch(fd, path.as_ptr(), IN_WATCH_MASK) };
            if watch < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let relative =
                first.and_then(|first| unix_events::inotify_folder_relative(first, directory));
            unix_events::record_inotify_folder(&mut prepared.folders, watch, relative);
        }
        Ok(prepared)
    }

    pub(super) fn run(
        prepared: Prepared,
        cancelled: &AtomicBool,
        deliver: &mut impl FnMut(String, u32),
    ) -> std::io::Result<()> {
        let mut buffer = [0_u8; 4096];
        while !cancelled.load(Ordering::Acquire) {
            let mut pollfd = PollFd {
                fd: prepared.fd,
                events: POLLIN,
                revents: 0,
            };
            let ready = unsafe { poll(&mut pollfd, 1, 100) };
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if ready < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ready == 0 {
                continue;
            }
            let read_bytes = unsafe { read(prepared.fd, buffer.as_mut_ptr(), buffer.len()) };
            if read_bytes < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    continue;
                }
                return Err(error);
            }
            for (relative, action) in unix_events::unix_watch_deliveries_from_inotify_bytes(
                &buffer[..read_bytes as usize],
                &prepared.folders,
            ) {
                deliver(relative, action);
            }
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::mpsc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn unix_workspace_root_watch_reports_change_or_overflow_then_cancels() {
        let directory = std::env::temp_dir().join(format!(
            "sage-workspace-notify-unix-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos(),
        ));
        fs::create_dir(&directory).expect("create test directory");
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_directory = directory.clone();
        let (notices, receive_notice) = mpsc::sync_channel(4);
        let (completed, receive_completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = (|| {
                let (directories, _truncated) =
                    unix_events::bounded_watch_directories(&worker_directory)
                        .map_err(|error| std::io::Error::other(error.code()))?;
                let prepared = prepare_watch(&directories)?;
                run_prepared(prepared, &worker_cancelled, &mut |relative, action| {
                    notices
                        .send((relative, action))
                        .expect("test receives watch notice")
                })
            })();
            completed
                .send(result)
                .expect("test receives worker completion");
        });

        let mut saw_notice = false;
        for index in 0..20 {
            fs::write(directory.join(format!("notes-{index}.md")), "created")
                .expect("create watched file");
            if receive_notice
                .recv_timeout(Duration::from_millis(150))
                .is_ok()
            {
                saw_notice = true;
                break;
            }
        }
        assert!(
            saw_notice,
            "unix root watch should report a named change or overflow resync"
        );

        cancelled.store(true, Ordering::Release);
        receive_completion
            .recv_timeout(Duration::from_secs(2))
            .expect("cancelled watch should stop promptly")
            .expect("cancelled watch should not fail");
        worker.join().expect("watch worker should not panic");
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[cfg(target_os = "linux")]
    fn linux_test_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sage-workspace-notify-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos(),
        ));
        fs::create_dir(&root).expect("create test root");
        root
    }

    /// Watches `root` as the Project watch does, runs `change` once the
    /// watches are in place, and returns the notices that arrive until
    /// `expected` does or two seconds pass.
    #[cfg(target_os = "linux")]
    fn linux_notices_after(
        root: &std::path::Path,
        change: impl FnOnce(),
        expected: &(String, u32),
    ) -> Vec<(String, u32)> {
        let (directories, truncated) =
            unix_events::bounded_watch_directories(root).expect("walk test root");
        assert!(!truncated);
        let prepared = prepare_watch(&directories).expect("watch test root");
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (notices, receive_notice) = mpsc::sync_channel(64);
        let worker = std::thread::spawn(move || {
            run_prepared(prepared, &worker_cancelled, &mut |relative, action| {
                let _ = notices.try_send((relative, action));
            })
        });
        change();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut received = Vec::new();
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(notice) = receive_notice.recv_timeout(left) else {
                break;
            };
            let done = &notice == expected;
            received.push(notice);
            if done {
                break;
            }
        }
        cancelled.store(true, Ordering::Release);
        worker
            .join()
            .expect("watch worker should not panic")
            .expect("cancelled watch should not fail");
        received
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_root_watch_reports_subfolder_change_at_its_real_path() {
        let root = linux_test_root("subfolder");
        fs::create_dir(root.join("src")).expect("create src");
        let expected = ("src/main.ts".to_string(), 1);
        let received = linux_notices_after(
            &root,
            || fs::write(root.join("src").join("main.ts"), "created").expect("write src/main.ts"),
            &expected,
        );
        fs::remove_dir_all(&root).expect("remove test root");
        assert!(
            received.contains(&expected),
            "expected a create notice for src/main.ts, got {received:?}"
        );
        assert!(
            received.iter().all(|(relative, _)| relative != "main.ts"),
            "a subfolder change must not be reported at the root, got {received:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_root_watch_covers_non_utf8_subfolder_with_resync() {
        use std::os::unix::ffi::OsStrExt;
        let root = linux_test_root("non-utf8");
        let folder = root.join(std::ffi::OsStr::from_bytes(b"bad\xff"));
        fs::create_dir(&folder).expect("create non-UTF-8 folder");
        let expected = (
            super::super::OVERFLOW_RELATIVE_PATH.to_string(),
            super::super::OVERFLOW_NOTIFY_ACTION,
        );
        let received = linux_notices_after(
            &root,
            || fs::write(folder.join("main.ts"), "created").expect("write in non-UTF-8 folder"),
            &expected,
        );
        fs::remove_dir_all(&root).expect("remove test root");
        assert_eq!(received, vec![expected]);
    }
}
