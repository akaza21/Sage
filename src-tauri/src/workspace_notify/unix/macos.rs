use super::{AtomicBool, Ordering, unix_events};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        raw::{c_int, c_void},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    ptr,
};

const O_NONBLOCK: c_int = 0x0004;
const O_NOFOLLOW: c_int = 0x0100;
const O_DIRECTORY: c_int = 0x10_0000;
const O_CLOEXEC: c_int = 0x100_0000;
const EVFILT_VNODE: i16 = -4;
const EV_ADD: u16 = 0x0001;
const EV_CLEAR: u16 = 0x0020;
const EV_ERROR: u16 = 0x4000;
const NOTE_DELETE: u32 = 0x0000_0001;
const NOTE_WRITE: u32 = 0x0000_0002;
const NOTE_EXTEND: u32 = 0x0000_0004;
const NOTE_ATTRIB: u32 = 0x0000_0008;
const NOTE_RENAME: u32 = 0x0000_0020;
const NOTE_REVOKE: u32 = 0x0000_0040;

#[repr(C)]
struct Kevent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut c_void,
}

impl Kevent {
    fn empty() -> Self {
        Self {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: ptr::null_mut(),
        }
    }
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

unsafe extern "C" {
    fn kqueue() -> c_int;
    fn openat(fd: c_int, path: *const i8, flags: c_int, ...) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn kevent(
        kq: c_int,
        changelist: *const Kevent,
        nchanges: c_int,
        eventlist: *mut Kevent,
        nevents: c_int,
        timeout: *const Timespec,
    ) -> c_int;
}

struct Target {
    path: PathBuf,
    file: Option<File>,
    missing_notified: bool,
}

pub(crate) struct Prepared {
    kq: OwnedFd,
    directories: Vec<File>,
    target: Option<Target>,
    initial_notice: Option<unix_events::UnixWatchKind>,
}

const WORKSPACE_MASK: u32 = NOTE_DELETE | NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_RENAME;

fn event_error(editor: bool, event: &Kevent) -> io::Result<()> {
    if editor && event.flags & EV_ERROR != 0 {
        return Err(io::Error::from_raw_os_error(event.data as c_int));
    }
    Ok(())
}

fn register(kq: &OwnedFd, file: &File, editor: bool) -> io::Result<()> {
    let change = Kevent {
        ident: file.as_raw_fd() as usize,
        filter: EVFILT_VNODE,
        flags: EV_ADD | EV_CLEAR,
        fflags: WORKSPACE_MASK | if editor { NOTE_REVOKE } else { 0 },
        ..Kevent::empty()
    };
    if unsafe { kevent(kq.as_raw_fd(), &change, 1, ptr::null_mut(), 0, ptr::null()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn prepare(directories: &[PathBuf]) -> io::Result<Prepared> {
    prepare_mode(directories, false)
}

fn prepare_mode(directories: &[PathBuf], editor: bool) -> io::Result<Prepared> {
    let fd = unsafe { kqueue() };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Own every descriptor before another fallible operation (including path conversion).
    let kq = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe {
        fcntl(
            kq.as_raw_fd(),
            2, /* F_SETFD */
            1, /* FD_CLOEXEC */
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut prepared = Prepared {
        kq,
        directories: Vec::new(),
        target: None,
        initial_notice: None,
    };
    for directory in directories {
        let file = if editor {
            open_editor_parent(directory)?
        } else {
            OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_CLOEXEC)
                .open(directory)?
        };
        register(&prepared.kq, &file, editor)?;
        prepared.directories.push(file);
    }
    Ok(prepared)
}

fn open_editor_parent(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(io::Error::other("editor parent must be absolute"));
    }
    let mut directory = File::open("/")?;
    for (index, component) in path.components().enumerate() {
        if index >= 256 {
            return Err(io::Error::other("editor parent depth bound"));
        }
        let Component::Normal(name) = component else {
            if component == Component::RootDir {
                continue;
            }
            return Err(io::Error::other("editor parent component"));
        };
        let name =
            CString::new(name.as_bytes()).map_err(|_| io::Error::other("editor parent name"))?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

pub(super) fn prepare_editor(path: &Path) -> io::Result<Prepared> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "editor watch parent"))?;
    let mut prepared = prepare_mode(&[parent.to_path_buf()], true)?;
    prepared.target = Some(Target {
        path: path.to_path_buf(),
        file: None,
        missing_notified: false,
    });
    // Parent is registered first, so replacement during target registration remains queued.
    prepared.initial_notice = prepared
        .reconcile_target()?
        .filter(|kind| *kind == unix_events::UnixWatchKind::Deleted);
    Ok(prepared)
}

impl Prepared {
    fn reconcile_target(&mut self) -> io::Result<Option<unix_events::UnixWatchKind>> {
        let target = self.target.as_mut().expect("editor target");
        let parent = &self.directories[0];
        // Validate the pathname still identifies the held parent. The subsequent open is
        // relative to that FD, so a rename/symlink swap after this check cannot redirect it.
        let current = target
            .path
            .parent()
            .expect("editor parent")
            .symlink_metadata()?;
        let held = parent.metadata()?;
        if !current.is_dir() || current.dev() != held.dev() || current.ino() != held.ino() {
            return Err(io::Error::other("editor watch parent identity changed"));
        }
        let name = CString::new(target.path.file_name().expect("editor name").as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "editor watch name"))?;
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::NotFound {
                return Err(error);
            }
            target.file = None;
            let notify = !target.missing_notified;
            target.missing_notified = true;
            return Ok(notify.then_some(unix_events::UnixWatchKind::Deleted));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "editor watch regular file",
            ));
        }
        let same_file = target
            .file
            .as_ref()
            .map(|old| old.metadata())
            .transpose()?
            .is_some_and(|old| old.dev() == metadata.dev() && old.ino() == metadata.ino());
        if !same_file {
            // At most one temporary candidate FD; drop the old registration before replacing it.
            target.file = None;
            register(&self.kq, &file, true)?;
            target.file = Some(file);
        }
        target.missing_notified = false;
        Ok(Some(unix_events::UnixWatchKind::Changed))
    }

    fn editor_delivery(
        &self,
        kind: unix_events::UnixWatchKind,
    ) -> Result<(String, u32), super::super::WorkspaceNotifyError> {
        let name = self
            .target
            .as_ref()
            .and_then(|target| target.path.file_name())
            .and_then(|name| name.to_str());
        unix_events::unix_watch_delivery(name, kind)
            .or_else(|_| unix_events::unix_watch_delivery(None, kind))
    }
}

pub(super) fn run(
    mut prepared: Prepared,
    cancelled: &AtomicBool,
    deliver: &mut impl FnMut(String, u32),
) -> io::Result<()> {
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: 100_000_000,
    };
    if !cancelled.load(Ordering::Acquire)
        && let Some(kind) = prepared.initial_notice.take()
        && let Ok((relative, action)) = prepared.editor_delivery(kind)
    {
        deliver(relative, action);
    }
    while !cancelled.load(Ordering::Acquire) {
        let mut event = Kevent::empty();
        let ready = unsafe {
            kevent(
                prepared.kq.as_raw_fd(),
                ptr::null(),
                0,
                &mut event,
                1,
                &timeout,
            )
        };
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 {
            continue;
        }
        event_error(prepared.target.is_some(), &event)?;
        let delivery = if prepared.target.is_some() {
            let parent_event = prepared
                .directories
                .iter()
                .any(|file| file.as_raw_fd() as usize == event.ident);
            if parent_event && event.fflags & (NOTE_DELETE | NOTE_RENAME | NOTE_REVOKE) != 0 {
                // Do not follow a renamed parent outside the already-resolved editor path.
                // The editor adapter emits one resync and ends this generation on failure.
                return Err(io::Error::other("editor watch parent invalidated"));
            }
            if event.fflags & NOTE_REVOKE != 0 {
                return Err(io::Error::other("editor watch target revoked"));
            }
            // Reattach BEFORE delivery so a write triggered by resync cannot race readiness.
            let Some(kind) = prepared.reconcile_target()? else {
                continue;
            };
            prepared.editor_delivery(kind)
        } else {
            // Workspace directory events retain the existing unnamed overflow/resync contract.
            unix_events::unix_watch_delivery(None, unix_events::UnixWatchKind::Changed)
        };
        if !cancelled.load(Ordering::Acquire)
            && let Ok((relative, action)) = delivery
        {
            deliver(relative, action);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
