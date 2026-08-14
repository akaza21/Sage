use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::entrypoint::protocol_argument;
use super::executable::VerifiedHelperExecutable;
use super::protocol::{
    BoundaryError, BoundaryStatus, HelperIdentity, OneShotSecret, REQUEST_PAYLOAD_BYTES_MAX,
    RESPONSE_FRAME_BYTES, SealedRequest, verify_response, wire_request,
};
use super::windows::{authority, channel};

const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryProbeReceipt {
    pub status: BoundaryStatus,
    pub child_pid: u32,
    pub pipe_name: String,
}

pub fn direct_non_elevated_probe(
    helper_path: &Path,
    opaque_request: &[u8],
    timeout: Duration,
) -> Result<BoundaryProbeReceipt, BoundaryError> {
    with_bounded_direct_payload(opaque_request, |bounded_request| {
        run_helper_with_payload(helper_path, timeout, |_helper, _secret| {
            Ok(bounded_request.to_vec())
        })
    })
}

fn with_bounded_direct_payload<T>(
    opaque_request: &[u8],
    accepted: impl FnOnce(&[u8]) -> Result<T, BoundaryError>,
) -> Result<T, BoundaryError> {
    if opaque_request.is_empty() || opaque_request.len() > REQUEST_PAYLOAD_BYTES_MAX {
        return Err(BoundaryError::Bound);
    }
    accepted(opaque_request)
}

pub(crate) fn run_helper_with_payload(
    helper_path: &Path,
    timeout: Duration,
    payload_builder: impl FnOnce(HelperIdentity, &OneShotSecret) -> Result<Vec<u8>, BoundaryError>,
) -> Result<BoundaryProbeReceipt, BoundaryError> {
    let started_at = Instant::now();
    if timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(BoundaryError::Deadline);
    }
    let deadline = started_at
        .checked_add(timeout)
        .ok_or(BoundaryError::Deadline)?;
    let (helper, caller_sid_sha256, pipe_name, secret, payload) =
        complete_before_deadline(deadline, || {
            let helper = VerifiedHelperExecutable::open(helper_path)?;
            let caller = authority::current_process_subject()?;
            if caller.elevated {
                return Err(BoundaryError::Authority);
            }
            let pipe_name = channel::fresh_name();
            let secret = OneShotSecret::generate();
            let payload = payload_builder(helper.identity(), &secret)?;
            Ok((helper, caller.sid_sha256, pipe_name, secret, payload))
        })?;
    let nonce = *Uuid::new_v4().as_bytes();
    let request = complete_before_deadline(deadline, || {
        SealedRequest::seal(
            std::process::id(),
            caller_sid_sha256,
            nonce,
            helper.identity(),
            &payload,
            &secret,
        )
    })?;
    let mut command = Command::new(helper.path());
    command
        .arg(protocol_argument())
        .arg(&pipe_name)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    ensure_before_deadline(deadline)?;
    let mut child = command.spawn().map_err(|_| BoundaryError::Io)?;
    let child_pid = child.id();
    let result = exchange(
        &mut child, child_pid, &pipe_name, &request, &secret, deadline,
    );
    match result {
        Ok(status) => {
            let exit = wait_child(&mut child, deadline)?;
            if !exit.success() {
                return Err(BoundaryError::Io);
            }
            Ok(BoundaryProbeReceipt {
                status,
                child_pid,
                pipe_name,
            })
        }
        Err(error) => {
            converge_child(&mut child)?;
            Err(error)
        }
    }
}

fn complete_before_deadline<T>(
    deadline: Instant,
    work: impl FnOnce() -> Result<T, BoundaryError>,
) -> Result<T, BoundaryError> {
    ensure_before_deadline(deadline)?;
    let result = work();
    ensure_before_deadline(deadline)?;
    result
}

fn ensure_before_deadline(deadline: Instant) -> Result<(), BoundaryError> {
    if Instant::now() >= deadline {
        return Err(BoundaryError::Deadline);
    }
    Ok(())
}

fn exchange(
    child: &mut Child,
    child_pid: u32,
    pipe_name: &str,
    request: &SealedRequest,
    secret: &OneShotSecret,
    deadline: Instant,
) -> Result<BoundaryStatus, BoundaryError> {
    if child.try_wait().map_err(|_| BoundaryError::Io)?.is_some() {
        return Err(BoundaryError::Io);
    }
    let pipe = channel::connect_client(pipe_name, child_pid, deadline)?;
    let mut transport = Vec::new();
    {
        let wire = wire_request(&mut transport, secret, request);
        channel::write_message(&pipe, wire.as_bytes())?;
    }
    let response = channel::read_message(&pipe, RESPONSE_FRAME_BYTES, deadline)?;
    verify_response(&response, request, secret)
}

fn wait_child(child: &mut Child, deadline: Instant) -> Result<ExitStatus, BoundaryError> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(_) => {
                converge_child(child)?;
                return Err(BoundaryError::Io);
            }
        }
        if Instant::now() >= deadline {
            converge_child(child)?;
            return Err(BoundaryError::Deadline);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

trait ChildProcessControl {
    fn kill_process(&mut self) -> std::io::Result<()>;
    fn poll_exit(&mut self) -> std::io::Result<Option<ExitStatus>>;
}

impl ChildProcessControl for Child {
    fn kill_process(&mut self) -> std::io::Result<()> {
        self.kill()
    }

    fn poll_exit(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.try_wait()
    }
}

fn converge_child(child: &mut Child) -> Result<(), BoundaryError> {
    converge_process(child, Instant::now() + PROCESS_CLEANUP_TIMEOUT)
}

fn converge_process(
    process: &mut impl ChildProcessControl,
    deadline: Instant,
) -> Result<(), BoundaryError> {
    let kill_failed = process.kill_process().is_err();
    loop {
        match process.poll_exit() {
            Ok(Some(_)) if kill_failed => return Err(BoundaryError::Io),
            Ok(Some(_)) => return Ok(()),
            Err(_) => return Err(BoundaryError::Io),
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            return Err(if kill_failed {
                BoundaryError::Io
            } else {
                BoundaryError::Deadline
            });
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use super::*;

    #[derive(Default)]
    struct KillFailureNeverExit {
        kill_calls: usize,
        poll_calls: usize,
    }

    impl ChildProcessControl for KillFailureNeverExit {
        fn kill_process(&mut self) -> io::Result<()> {
            self.kill_calls += 1;
            Err(io::Error::other("injected kill failure"))
        }

        fn poll_exit(&mut self) -> io::Result<Option<ExitStatus>> {
            self.poll_calls += 1;
            Ok(None)
        }
    }

    #[test]
    fn kill_failure_and_never_exit_return_without_blocking_or_hiding_the_kill_error() {
        let mut process = KillFailureNeverExit::default();

        let result = converge_process(&mut process, Instant::now());

        assert_eq!(result, Err(BoundaryError::Io));
        assert_eq!(process.kill_calls, 1);
        assert_eq!(process.poll_calls, 1);
    }

    #[test]
    fn invalid_direct_probe_payload_is_rejected_before_helper_identity_setup() {
        let missing_helper = std::env::temp_dir().join(format!(
            "sage-missing-helper-{}.exe",
            Uuid::new_v4().simple()
        ));
        let retention_calls = Cell::new(0);

        for payload in [&[][..], &vec![0; REQUEST_PAYLOAD_BYTES_MAX + 1]] {
            assert_eq!(
                with_bounded_direct_payload(payload, |_| {
                    retention_calls.set(retention_calls.get() + 1);
                    Ok(())
                }),
                Err(BoundaryError::Bound)
            );
            assert_eq!(
                direct_non_elevated_probe(&missing_helper, payload, Duration::from_secs(1)),
                Err(BoundaryError::Bound)
            );
        }
        assert_eq!(retention_calls.get(), 0);
    }

    #[test]
    fn payload_builder_cannot_extend_the_absolute_caller_deadline() {
        let builder_calls = Cell::new(0);
        let deadline = Instant::now() + Duration::from_millis(50);

        let result = complete_before_deadline(deadline, || {
            builder_calls.set(builder_calls.get() + 1);
            std::thread::sleep(Duration::from_millis(75));
            Ok(())
        });

        assert_eq!(result, Err(BoundaryError::Deadline));
        assert_eq!(builder_calls.get(), 1);
    }
}
