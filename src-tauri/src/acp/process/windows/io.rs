use std::fs::File;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::acp::limits::LIMITS;
use crate::acp::redaction::{SafeDiagnostic, SafeStderrDiagnostic, StderrRing};
use crate::acp::transport::{
    Frame, FrameError, FrameWriteOutcome, IncrementalFrameDecoder, OutboundFrame,
    write_json_line_blocking,
};

use super::super::supervisor::LaunchError;

/// Independently owned stdin authority. Blocking writes never hold the tree
/// control lock, so cancellation can always dispatch concurrently.
pub(crate) struct StdinEndpoint(Option<File>);

impl StdinEndpoint {
    pub(super) fn new(file: File) -> Self {
        Self(Some(file))
    }

    pub(crate) fn write_frame(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let Some(output) = self.0.as_mut() else {
            return FrameWriteOutcome::ZeroByteFailure;
        };
        write_json_line_blocking(output, frame)
    }

    pub(crate) fn close(&mut self) {
        self.0.take();
    }
}

/// Independently owned bounded stdout reader.
pub(crate) struct StdoutEndpoint {
    input: File,
    decoder: IncrementalFrameDecoder,
}

impl StdoutEndpoint {
    pub(super) fn new(file: File) -> Self {
        Self {
            input: file,
            decoder: IncrementalFrameDecoder::new(),
        }
    }

    pub(crate) fn read(&mut self, target: &mut [u8]) -> Result<usize, LaunchError> {
        if target.len() as u64 > LIMITS.wire.frame_bytes {
            return Err(LaunchError::StdioLimit);
        }
        self.input.read(target).map_err(|_| LaunchError::Io)
    }

    /// Reads exactly one bounded frame while preserving any additional frames
    /// already present in the same native pipe chunk for the next call.
    pub(crate) fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            if let Some(frame) = self.decoder.drain_pending()? {
                return Ok(Some(frame));
            }
            let read = self.input.read(&mut chunk).map_err(|_| FrameError::Io)?;
            if read == 0 {
                return self.decoder.finish();
            }
            if let Some(frame) = self.decoder.push_chunk(&chunk[..read])? {
                return Ok(Some(frame));
            }
        }
    }
}

pub(super) struct StderrDrain {
    ring: Arc<Mutex<StderrRing>>,
    thread: Mutex<DrainThread>,
}

struct DrainThread {
    handle: Option<JoinHandle<()>>,
    completion: mpsc::Receiver<bool>,
    terminal: Option<Result<(), LaunchError>>,
}

impl StderrDrain {
    pub(super) fn spawn(mut stderr: File) -> Result<Self, LaunchError> {
        let ring = Arc::new(Mutex::new(StderrRing::new(
            usize::try_from(LIMITS.process.stderr_ring_bytes).unwrap_or(usize::MAX),
            usize::try_from(LIMITS.process.stderr_line_bytes).unwrap_or(usize::MAX),
        )));
        let drain_ring = Arc::clone(&ring);
        let thread = DrainThread::spawn(move || {
            let mut buffer = [0_u8; 8 * 1024];
            loop {
                let read = match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let Ok(mut ring) = drain_ring.lock() else {
                    break;
                };
                ring.ingest(&buffer[..read]);
            }
        })?;
        Ok(Self {
            ring,
            thread: Mutex::new(thread),
        })
    }

    pub(super) fn summary(&self) -> Result<SafeDiagnostic, LaunchError> {
        self.ring
            .lock()
            .map(|ring| ring.safe_summary())
            .map_err(|_| LaunchError::ContainmentFailure)
    }

    pub(super) fn diagnostic(&self) -> Result<SafeStderrDiagnostic, LaunchError> {
        self.ring
            .lock()
            .map(|ring| ring.safe_diagnostic())
            .map_err(|_| LaunchError::ContainmentFailure)
    }

    pub(super) fn wait_until(&self, deadline: Instant) -> Result<(), LaunchError> {
        self.thread
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .wait_until(deadline)
    }
}

impl DrainThread {
    fn spawn(task: impl FnOnce() + Send + 'static) -> Result<Self, LaunchError> {
        let (completed, completion) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("sage-acp-stderr".to_owned())
            .spawn(move || {
                let clean = catch_unwind(AssertUnwindSafe(task)).is_ok();
                let _ = completed.send(clean);
            })
            .map_err(|_| LaunchError::Io)?;
        Ok(Self {
            handle: Some(handle),
            completion,
            terminal: None,
        })
    }

    fn wait_until(&mut self, deadline: Instant) -> Result<(), LaunchError> {
        if let Some(result) = self.terminal {
            return result;
        }
        let result = match self
            .completion
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(true) => {
                self.handle.take();
                Ok(())
            }
            Ok(false) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.handle.take();
                Err(LaunchError::ContainmentFailure)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(LaunchError::Deadline),
        };
        if !matches!(result, Err(LaunchError::Deadline)) {
            self.terminal = Some(result);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use uuid::Uuid;

    use super::*;

    #[test]
    fn blocking_stdout_preserves_multiple_frames_from_one_read() {
        let path = std::env::temp_dir().join(format!("sage-acp-stdout-{}", Uuid::new_v4()));
        fs::write(&path, b"one\r\ntwo\n").unwrap();
        let mut stdout = StdoutEndpoint::new(File::open(&path).unwrap());
        assert_eq!(stdout.read_frame().unwrap().unwrap().as_bytes(), b"one");
        assert_eq!(stdout.read_frame().unwrap().unwrap().as_bytes(), b"two");
        assert!(stdout.read_frame().unwrap().is_none());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn stalled_stderr_completion_returns_at_deadline_and_can_be_retried() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_gate = Arc::clone(&gate);
        let mut drain = DrainThread::spawn(move || {
            let (released, ready) = &*thread_gate;
            let mut released = released.lock().unwrap();
            while !*released {
                released = ready.wait(released).unwrap();
            }
        })
        .unwrap();

        let started = Instant::now();
        assert_eq!(
            drain.wait_until(started + Duration::from_millis(20)),
            Err(LaunchError::Deadline)
        );
        assert!(started.elapsed() < Duration::from_secs(1));

        let (released, ready) = &*gate;
        *released.lock().unwrap() = true;
        ready.notify_all();
        assert_eq!(
            drain.wait_until(Instant::now() + Duration::from_secs(1)),
            Ok(())
        );
    }
}
