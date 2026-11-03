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
    linux::prepare(directories, true)
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
    linux::prepare(&[parent.to_path_buf()], false)
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
        path::PathBuf,
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
    const IN_TREE_MASK: u32 =
        IN_WATCH_MASK | unix_events::IN_DELETE_SELF | unix_events::IN_MOVE_SELF;
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
        fn inotify_rm_watch(fd: c_int, wd: c_int) -> c_int;
        fn close(fd: c_int) -> c_int;
        fn read(fd: c_int, buf: *mut u8, count: usize) -> isize;
        fn poll(fds: *mut PollFd, nfds: u64, timeout: c_int) -> c_int;
    }

    pub(crate) struct Prepared {
        fd: c_int,
        root: PathBuf,
        folders: unix_events::InotifyFolders,
    }

    impl Drop for Prepared {
        fn drop(&mut self) {
            unsafe { close(self.fd) };
        }
    }

    /// Watches `directories`, the first of which is the root. With
    /// `subfolders`, they are the root's bounded folder tree, which `run`
    /// watches again whenever its shape changes.
    pub(super) fn prepare(directories: &[PathBuf], subfolders: bool) -> std::io::Result<Prepared> {
        let fd = unsafe { inotify_init1(IN_CLOEXEC | IN_NONBLOCK) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Dropping `prepared` on any early return closes the descriptor.
        let mut prepared = Prepared {
            fd,
            root: directories.first().cloned().unwrap_or_default(),
            folders: unix_events::InotifyFolders::new(subfolders),
        };
        prepared.folders = add_watches(fd, directories, subfolders, true)?;
        Ok(prepared)
    }

    /// Adds a watch for each directory. Re-adding a folder that is already
    /// watched returns its existing descriptor. Only the root must succeed
    /// unless `strict`: a folder that vanished during the walk shows up as
    /// another tree change.
    fn add_watches(
        fd: c_int,
        directories: &[PathBuf],
        subfolders: bool,
        strict: bool,
    ) -> std::io::Result<unix_events::InotifyFolders> {
        let mask = if subfolders {
            IN_TREE_MASK
        } else {
            IN_WATCH_MASK
        };
        let mut folders = unix_events::InotifyFolders::new(subfolders);
        let first = directories.first();
        for (index, directory) in directories.iter().enumerate() {
            let path = CString::new(directory.as_os_str().as_bytes()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "notify path")
            })?;
            let watch = unsafe { inotify_add_watch(fd, path.as_ptr(), mask) };
            if watch < 0 {
                if strict || index == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                continue;
            }
            let relative =
                first.and_then(|first| unix_events::inotify_folder_relative(first, directory));
            folders.record(watch, relative);
        }
        Ok(folders)
    }

    /// Watches the root's bounded folder tree as it is now and removes the
    /// watches of folders that left it. The walk keeps the 64-folder cap.
    fn rewatch(prepared: &mut Prepared) -> std::io::Result<()> {
        let (directories, _truncated) = unix_events::bounded_watch_directories(&prepared.root)
            .map_err(|error| std::io::Error::other(error.code()))?;
        let next = add_watches(prepared.fd, &directories, true, false)?;
        for watch in prepared.folders.dropped_from(&next) {
            unsafe { inotify_rm_watch(prepared.fd, watch) };
        }
        prepared.folders = next;
        Ok(())
    }

    pub(super) fn run(
        mut prepared: Prepared,
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
            let batch = unix_events::unix_watch_deliveries_from_inotify_bytes(
                &buffer[..read_bytes as usize],
                &prepared.folders,
            );
            // Watch the new tree before the resync goes out, so a rescan
            // that follows it cannot miss a change in a new folder.
            if batch.rewatch && prepared.folders.subfolders() {
                rewatch(&mut prepared)?;
            }
            for (relative, action) in batch.deliveries {
                deliver(relative, action);
            }
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "unix/linux_tests.rs"]
mod linux_tests;

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
}
