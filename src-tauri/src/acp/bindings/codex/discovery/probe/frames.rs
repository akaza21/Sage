//! The probe's stdout, decoded into frames on a reader thread that teardown
//! can stop even while a process outside the probe's tree holds the pipe.

use std::{
    io::{self, Read},
    process::ChildStdout,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::acp::transport::{Frame, FrameError, IncrementalFrameDecoder};

/// How long the reader waits for output before checking whether to stop.
const READY_POLL: Duration = Duration::from_millis(10);

pub(super) type FrameSender = mpsc::SyncSender<Result<Option<Frame>, FrameError>>;

/// One probe resource not yet released, such as a reader thread or a root
/// left to a reaper, counted until it is dropped.
pub(super) struct Lease(Arc<AtomicUsize>);

impl Lease {
    pub(super) fn take(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(count))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct Reader {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

/// Reads at most `limit` bytes of `stdout` into `sender`, holding `lease`
/// until the thread ends.
pub(super) fn spawn(
    stdout: ChildStdout,
    limit: u64,
    sender: FrameSender,
    lease: Lease,
) -> io::Result<Reader> {
    let stop = Arc::new(AtomicBool::new(false));
    let output = Stoppable {
        stdout,
        stop: Arc::clone(&stop),
    };
    let handle = thread::Builder::new().spawn(move || {
        let _lease = lease;
        read_frames(output.take(limit), &sender);
    })?;
    Ok(Reader { handle, stop })
}

impl Reader {
    /// The reader ends at its next wait for output, with an error frame.
    pub(super) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Joins a reader that has ended; one still running is left to end on
    /// its own, still counted by its lease.
    pub(super) fn release(self) {
        if self.handle.is_finished() {
            let _ = self.handle.join();
        }
    }
}

struct Stoppable {
    stdout: ChildStdout,
    stop: Arc<AtomicBool>,
}

impl Read for Stoppable {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stop.load(Ordering::Acquire) {
                return Err(io::Error::other("the probe's output reader was stopped"));
            }
            if readable(&self.stdout)? {
                return self.stdout.read(buffer);
            }
        }
    }
}

/// True when a read would not block: output, end of stream or an error.
#[cfg(unix)]
fn readable(stdout: &ChildStdout) -> io::Result<bool> {
    use std::os::fd::AsRawFd;

    let mut pipe = libc::pollfd {
        fd: stdout.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let wait = libc::c_int::try_from(READY_POLL.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: one valid pollfd for a descriptor this reader owns.
    match unsafe { libc::poll(&raw mut pipe, 1, wait) } {
        0 => Ok(false),
        ready if ready > 0 => Ok(true),
        _ => {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(error)
            }
        }
    }
}

/// True when a read would not block: output, end of stream or an error.
#[cfg(windows)]
fn readable(stdout: &ChildStdout) -> io::Result<bool> {
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};

    use windows_sys::Win32::{Foundation::HANDLE, System::Pipes::PeekNamedPipe};

    let mut available = 0_u32;
    // SAFETY: peeks, without reading, the pipe handle this reader owns.
    let peeked = unsafe {
        PeekNamedPipe(
            stdout.as_raw_handle() as HANDLE,
            null_mut(),
            0,
            null_mut(),
            &raw mut available,
            null_mut(),
        )
    };
    if peeked == 0 || available > 0 {
        return Ok(true);
    }
    thread::sleep(READY_POLL);
    Ok(false)
}

/// Sends each frame, then the end of stream or the first error, and stops.
fn read_frames(mut stdout: impl Read, sender: &FrameSender) {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let next = match decoder.drain_pending() {
            Ok(Some(frame)) => Ok(Some(frame)),
            Ok(None) => match stdout.read(&mut chunk) {
                Ok(0) => decoder.finish(),
                Ok(read) => match decoder.push_chunk(&chunk[..read]) {
                    Ok(None) => continue,
                    other => other,
                },
                Err(_) => Err(FrameError::Io),
            },
            Err(error) => Err(error),
        };
        let last = !matches!(next, Ok(Some(_)));
        if sender.send(next).is_err() || last {
            return;
        }
    }
}
