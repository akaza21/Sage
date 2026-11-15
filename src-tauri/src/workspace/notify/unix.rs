use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{
    WorkspaceNotifyError, apply_unwatch_if_generation, batch::NoticeSink, reject, unix_events,
    watch::root_delivery,
};

pub(super) fn spawn_workspace_root_watch(
    sink: Arc<NoticeSink>,
    project_id: String,
    generation: u32,
    root_id: String,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    let (directories, truncated) = unix_events::bounded_watch_directories(&path)?;
    let prepared = prepare_watch(&directories).map_err(|_| reject("notify-substituted"))?;
    let mut deliver = root_delivery(
        project_id.clone(),
        generation,
        root_id,
        &path,
        Arc::clone(&cancelled),
        sink,
    );
    std::thread::Builder::new()
        .name("sage-workspace-notify".into())
        .spawn(move || {
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
    use crate::workspace::{
        directory_skip::skip_workspace_directory_name, file_walk::walk_over_cap,
    };
    use std::{
        collections::VecDeque,
        ffi::{CStr, CString, OsStr},
        fs::{File, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd, IntoRawFd},
            raw::{c_char, c_int},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
            },
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
    const FOLDER_FLAGS: c_int = libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK;

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

    type FolderId = (u64, u64);

    pub(crate) struct Prepared {
        fd: c_int,
        root: PathBuf,
        /// A rewatch that finds anything else at the root's path stops the watch.
        root_id: Option<FolderId>,
        folders: unix_events::InotifyFolders,
        max_entries: u64,
    }

    impl Drop for Prepared {
        fn drop(&mut self) {
            unsafe { close(self.fd) };
        }
    }

    pub(super) fn prepare(directories: &[PathBuf], subfolders: bool) -> std::io::Result<Prepared> {
        prepare_within(directories, subfolders, unix_events::MAX_UNIX_WATCH_ENTRIES)
    }

    pub(super) fn prepare_within(
        directories: &[PathBuf],
        subfolders: bool,
        max_entries: u64,
    ) -> std::io::Result<Prepared> {
        let fd = unsafe { inotify_init1(IN_CLOEXEC | IN_NONBLOCK) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut prepared = Prepared {
            fd,
            root: directories.first().cloned().unwrap_or_default(),
            root_id: None,
            folders: unix_events::InotifyFolders::new(subfolders),
            max_entries,
        };
        if subfolders {
            let (folders, root_id) = watch_tree(&prepared, true)?;
            prepared.folders = folders;
            prepared.root_id = Some(root_id);
            return Ok(prepared);
        }
        let path = CString::new(prepared.root.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "notify path"))?;
        let watch = unsafe { inotify_add_watch(fd, path.as_ptr(), IN_WATCH_MASK) };
        if watch < 0 {
            return Err(std::io::Error::last_os_error());
        }
        prepared.folders.record(watch, Some(String::new()));
        Ok(prepared)
    }

    /// Each subfolder is opened from its open parent without following symlinks
    /// and watched through that descriptor, so symlinks are refused and nothing swapped in
    /// after a folder is opened is watched in its place.
    /// Past the caps it still succeeds, partly watched; the tree-change resync covers it.
    /// Initial setup fails on descendant-watch errors; rewatch tolerates them.
    fn watch_tree(
        prepared: &Prepared,
        strict: bool,
    ) -> std::io::Result<(unix_events::InotifyFolders, FolderId)> {
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(FOLDER_FLAGS)
            .open(&prepared.root)?;
        let metadata = root.metadata()?;
        let root_id = (metadata.dev(), metadata.ino());
        if prepared.root_id.is_some_and(|known| known != root_id) {
            return Err(std::io::Error::other("notify-substituted"));
        }
        let mut folders = unix_events::InotifyFolders::new(true);
        folders.record(add_watch_on(prepared.fd, &root)?, Some(String::new()));
        let mut queue = VecDeque::from([(root, Some(String::new()))]);
        let mut found = 1_usize;
        let mut examined = 0_u64;
        while let Some((folder, relative)) = queue.pop_front() {
            let Some(mut listing) = Listing::open(folder) else {
                continue;
            };
            while let Some((name, kind)) = listing.next_entry() {
                examined += 1;
                if walk_over_cap(examined, prepared.max_entries) {
                    return Ok((folders, root_id));
                }
                if !matches!(kind, libc::DT_DIR | libc::DT_UNKNOWN)
                    || skip_workspace_directory_name(OsStr::from_bytes(name.to_bytes()))
                {
                    continue;
                }
                let Some(child) = listing.open_folder(&name) else {
                    continue;
                };
                if found >= unix_events::MAX_UNIX_WATCH_DIRECTORIES {
                    return Ok((folders, root_id));
                }
                let watch = match add_watch_on(prepared.fd, &child) {
                    Ok(watch) => watch,
                    Err(error) if strict => return Err(error),
                    Err(_) => continue,
                };
                found += 1;
                let child_relative =
                    unix_events::inotify_child_relative(relative.as_deref(), name.to_bytes());
                folders.record(watch, child_relative.clone());
                queue.push_back((child, child_relative));
            }
        }
        Ok((folders, root_id))
    }

    fn add_watch_on(fd: c_int, folder: &File) -> std::io::Result<c_int> {
        let path = CString::new(format!("/proc/self/fd/{}", folder.as_raw_fd()))
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "notify path"))?;
        let watch = unsafe { inotify_add_watch(fd, path.as_ptr(), IN_TREE_MASK) };
        if watch < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(watch)
    }

    struct Listing(*mut libc::DIR);

    impl Listing {
        fn open(folder: File) -> Option<Self> {
            let fd = folder.into_raw_fd();
            let stream = unsafe { libc::fdopendir(fd) };
            if stream.is_null() {
                unsafe { close(fd) };
                return None;
            }
            Some(Self(stream))
        }

        fn next_entry(&mut self) -> Option<(CString, u8)> {
            loop {
                let entry = unsafe { libc::readdir(self.0) };
                if entry.is_null() {
                    return None;
                }
                let (name, kind) =
                    unsafe { (CStr::from_ptr((*entry).d_name.as_ptr()), (*entry).d_type) };
                if name != c"." && name != c".." {
                    return Some((name.to_owned(), kind));
                }
            }
        }

        fn open_folder(&self, name: &CStr) -> Option<File> {
            let fd = unsafe {
                libc::openat(
                    libc::dirfd(self.0),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | FOLDER_FLAGS,
                )
            };
            (fd >= 0).then(|| unsafe { File::from_raw_fd(fd) })
        }
    }

    impl Drop for Listing {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }

    fn rewatch(prepared: &mut Prepared) -> std::io::Result<()> {
        let (next, _) = watch_tree(prepared, false)?;
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

#[cfg(test)]
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
