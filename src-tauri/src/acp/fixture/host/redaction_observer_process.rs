//! Bounded watchdog observations; never include child output or evidence paths.
use std::{
    io::Read,
    process::{Child, ExitStatus},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::{NODE_OUTPUT_BYTES, NODE_TIMEOUT};
use crate::acp::limits::LIMITS;

const SINK_EMITTED: &[u8] = b"sage-redaction:sink-emitted\n";

#[derive(Clone, Copy, Default)]
struct PipeProgress {
    bytes: u64,
    eof: bool,
}

#[derive(Default)]
struct Progress {
    pipes: [PipeProgress; 2],
    sink_emitted: bool,
}

pub(super) struct NodeStream {
    receiver: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    index: usize,
}

pub(super) struct ChildGuard {
    child: Child,
    completed: bool,
    started: Instant,
    spawned: Instant,
    state: (&'static str, Instant),
    progress: Arc<Mutex<Progress>>,
}

impl ChildGuard {
    pub(super) fn new(child: Child, started: Instant, spawned: Instant) -> Self {
        Self {
            child,
            completed: false,
            started,
            spawned,
            state: ("spawned", spawned),
            progress: Arc::new(Mutex::new(Progress::default())),
        }
    }

    pub(super) fn read_streams(&mut self) -> (NodeStream, NodeStream) {
        let stdout = read_node_stream(
            self.child.stdout.take().unwrap(),
            0,
            Arc::clone(&self.progress),
        );
        let stderr = read_node_stream(
            self.child.stderr.take().unwrap(),
            1,
            Arc::clone(&self.progress),
        );
        (stdout, stderr)
    }

    pub(super) fn timing(&self) -> String {
        let now = Instant::now();
        format!(
            "elapsed={:.3?}, spawn={:.3?}, post_spawn={:.3?}, limit={:.3?}",
            now.duration_since(self.started),
            self.spawned.duration_since(self.started),
            now.duration_since(self.spawned),
            NODE_TIMEOUT,
        )
    }

    fn transition(&mut self, state: &'static str) {
        self.state = (state, Instant::now());
    }

    // Capture before Drop terminates the child. All fields are fixed labels or
    // numbers. A missing emission marker is unknown, never proof of non-emission.
    fn timeout_observation(&mut self) -> String {
        let child = match self.child.try_wait() {
            Ok(Some(status)) => {
                self.completed = true;
                format!("exited exit_code={:?}", status.code())
            }
            Ok(None) => "alive".to_owned(),
            Err(error) => format!("unavailable os_error={:?}", error.raw_os_error()),
        };
        let cpu = child_cpu(&self.child);
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let [stdout, stderr] = progress.pipes;
        format!(
            "{}; child={child}; {cpu}; stdout_bytes={} stdout_eof={} stdout_capped={}; \
             stderr_bytes={} stderr_eof={} stderr_capped={}; last_state={} state_at={:.3?} \
             state_age={:.3?}; sink_emitted={}",
            self.timing(),
            stdout.bytes,
            stdout.eof,
            stdout.bytes > NODE_OUTPUT_BYTES,
            stderr.bytes,
            stderr.eof,
            stderr.bytes > NODE_OUTPUT_BYTES,
            self.state.0,
            self.state.1.duration_since(self.started),
            self.state.1.elapsed(),
            if progress.sink_emitted {
                "yes"
            } else {
                "not-observed"
            },
        )
    }

    pub(super) fn wait_until(&mut self, deadline: Instant) -> ExitStatus {
        self.transition("waiting-child");
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.completed = true;
                    self.transition("child-exited");
                    return status;
                }
                Ok(None) => {}
                Err(_) => panic!("curated observer status unavailable"),
            }
            assert!(
                Instant::now() < deadline,
                "curated observer exceeded its time limit; {}",
                self.timeout_observation()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn receive_stream(&mut self, stream: NodeStream, deadline: Instant) -> Vec<u8> {
        let name = if stream.index == 0 {
            "stdout"
        } else {
            "stderr"
        };
        self.transition(if stream.index == 0 {
            "waiting-stdout"
        } else {
            "waiting-stderr"
        });
        stream
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|_| {
                panic!(
                    "curated observer {name} did not close; {}",
                    self.timeout_observation()
                )
            })
            .unwrap_or_else(|_| panic!("curated observer {name} could not be read"))
    }
}

fn read_node_stream<R>(mut reader: R, index: usize, progress: Arc<Mutex<Progress>>) -> NodeStream
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 8192];
        let result = loop {
            let remaining = (NODE_OUTPUT_BYTES + 1) as usize - output.len();
            if remaining == 0 {
                break Ok(output);
            }
            let capacity = remaining.min(buffer.len());
            let read = match reader.read(&mut buffer[..capacity]) {
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => break Err(error),
            };
            let previous_len = output.len();
            output.extend_from_slice(&buffer[..read]);
            // Include the previous suffix so a marker split across reads is seen.
            let emitted = index == 1
                && output[previous_len.saturating_sub(SINK_EMITTED.len() - 1)..]
                    .windows(SINK_EMITTED.len())
                    .any(|window| window == SINK_EMITTED);
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.pipes[index] = PipeProgress {
                bytes: output.len() as u64,
                eof: read == 0,
            };
            progress.sink_emitted |= emitted;
            if read == 0 {
                break Ok(output);
            }
        };
        let _ = sender.send(result);
    });
    NodeStream { receiver, index }
}

#[cfg(windows)]
fn child_cpu(child: &Child) -> String {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};

    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: std::process::Child retains this query-capable handle even after
    // try_wait observes exit. All FILETIME outputs are writable; no PID is reopened.
    if unsafe {
        GetProcessTimes(
            child.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return format!(
            "cpu=unavailable os_error={:?}",
            std::io::Error::last_os_error().raw_os_error()
        );
    }
    let millis = |time: FILETIME| {
        ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) / 10_000
    };
    format!("userMs={} kernelMs={}", millis(user), millis(kernel))
}

#[cfg(not(windows))]
fn child_cpu(_child: &Child) -> String {
    "cpu=unavailable(platform)".to_owned()
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.child.kill();
            let deadline =
                Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.tree_termination);
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => thread::sleep(Duration::from_millis(10)),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "redaction_observer_process_tests.rs"]
mod tests;
