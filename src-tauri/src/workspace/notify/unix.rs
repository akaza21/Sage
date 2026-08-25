use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tauri::{AppHandle, Emitter};

use super::{
    apply_unwatch_if_generation, deliver_workspace_notify_action, reject, unix_events,
    WorkspaceNotifyError, WORKSPACE_NOTIFY_EVENT,
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
            if truncated {
                if let Ok((relative, action)) =
                    unix_events::unix_watch_delivery(None, unix_events::UnixWatchKind::Overflow)
                {
                    deliver(relative, action);
                }
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
mod macos {
    use super::{unix_events, AtomicBool, Ordering};
    use std::{
        ffi::CString,
        os::raw::{c_int, c_void},
        path::Path,
        ptr,
        time::Duration,
    };

    const O_RDONLY: c_int = 0;
    const O_DIRECTORY: c_int = 0x10_0000;
    const O_CLOEXEC: c_int = 0x100_0000;
    const EVFILT_VNODE: i16 = -4;
    const EV_ADD: u16 = 0x0001;
    const EV_CLEAR: u16 = 0x0020;
    const NOTE_DELETE: u32 = 0x0000_0001;
    const NOTE_WRITE: u32 = 0x0000_0002;
    const NOTE_EXTEND: u32 = 0x0000_0004;
    const NOTE_ATTRIB: u32 = 0x0000_0008;
    const NOTE_RENAME: u32 = 0x0000_0020;

    #[repr(C)]
    struct Kevent {
        ident: usize,
        filter: i16,
        flags: u16,
        fflags: u32,
        data: isize,
        udata: *mut c_void,
    }

    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }

    unsafe extern "C" {
        fn open(path: *const i8, oflag: c_int, ...) -> c_int;
        fn close(fd: c_int) -> c_int;
        fn kqueue() -> c_int;
        fn kevent(
            kq: c_int,
            changelist: *const Kevent,
            nchanges: c_int,
            eventlist: *mut Kevent,
            nevents: c_int,
            timeout: *const Timespec,
        ) -> c_int;
    }

    pub(crate) struct Prepared {
        kq: c_int,
        dir_fds: Vec<c_int>,
    }

    impl Drop for Prepared {
        fn drop(&mut self) {
            unsafe {
                close(self.kq);
                for dir_fd in &self.dir_fds {
                    close(*dir_fd);
                }
            }
        }
    }

    pub(super) fn prepare(directories: &[std::path::PathBuf]) -> std::io::Result<Prepared> {
        let kq = unsafe { kqueue() };
        if kq < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut dir_fds = Vec::new();
        for directory in directories {
            let path = CString::new(directory.to_string_lossy().as_bytes()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "notify path")
            })?;
            let dir_fd = unsafe { open(path.as_ptr(), O_RDONLY | O_DIRECTORY | O_CLOEXEC) };
            if dir_fd < 0 {
                let error = std::io::Error::last_os_error();
                unsafe {
                    close(kq);
                    for fd in &dir_fds {
                        close(*fd);
                    }
                }
                return Err(error);
            }
            let change = Kevent {
                ident: dir_fd as usize,
                filter: EVFILT_VNODE,
                flags: EV_ADD | EV_CLEAR,
                fflags: NOTE_DELETE | NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_RENAME,
                data: 0,
                udata: ptr::null_mut(),
            };
            let registered = unsafe { kevent(kq, &change, 1, ptr::null_mut(), 0, ptr::null()) };
            if registered < 0 {
                let error = std::io::Error::last_os_error();
                unsafe {
                    close(kq);
                    close(dir_fd);
                    for fd in &dir_fds {
                        close(*fd);
                    }
                }
                return Err(error);
            }
            dir_fds.push(dir_fd);
        }
        Ok(Prepared { kq, dir_fds })
    }

    pub(super) fn run(
        prepared: Prepared,
        cancelled: &AtomicBool,
        deliver: &mut impl FnMut(String, u32),
    ) -> std::io::Result<()> {
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: Duration::from_millis(100).as_nanos() as i64,
        };
        while !cancelled.load(Ordering::Acquire) {
            let mut event = Kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: ptr::null_mut(),
            };
            let ready = unsafe { kevent(prepared.kq, ptr::null(), 0, &mut event, 1, &timeout) };
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if ready < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ready == 0 {
                continue;
            }
            if let Ok((relative, action)) =
                unix_events::unix_watch_delivery(None, unix_events::UnixWatchKind::Changed)
            {
                deliver(relative, action);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{unix_events, AtomicBool, Ordering};
    use std::{
        ffi::CString,
        os::raw::{c_char, c_int},
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
        for directory in directories {
            let path = CString::new(directory.to_string_lossy().as_bytes()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "notify path")
            })?;
            let watch = unsafe { inotify_add_watch(fd, path.as_ptr(), IN_WATCH_MASK) };
            if watch < 0 {
                let error = std::io::Error::last_os_error();
                unsafe { close(fd) };
                return Err(error);
            }
        }
        Ok(Prepared { fd })
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
}
