use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::entrypoint::protocol_argument;
use super::executable::VerifiedHelperExecutable;
use super::protocol::{
    BoundaryError, BoundaryStatus, OneShotSecret, RESPONSE_FRAME_BYTES, SealedRequest,
    verify_response, wire_request,
};
use super::windows::{authority, channel};

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
    if timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(BoundaryError::Deadline);
    }
    let helper = VerifiedHelperExecutable::open(helper_path)?;
    let caller = authority::current_process_subject()?;
    if caller.elevated {
        return Err(BoundaryError::Authority);
    }
    let pipe_name = channel::fresh_name();
    let secret = OneShotSecret::generate();
    let nonce = *Uuid::new_v4().as_bytes();
    let request = SealedRequest::seal(
        std::process::id(),
        caller.sid_sha256,
        nonce,
        helper.identity(),
        opaque_request,
        &secret,
    )?;
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
    let mut child = command.spawn().map_err(|_| BoundaryError::Io)?;
    let child_pid = child.id();
    let deadline = Instant::now() + timeout;
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
            converge_child(&mut child);
            Err(error)
        }
    }
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
    channel::write_message(&pipe, &wire_request(secret, request))?;
    let response = channel::read_message(&pipe, RESPONSE_FRAME_BYTES, deadline)?;
    verify_response(&response, request, secret)
}

fn wait_child(child: &mut Child, deadline: Instant) -> Result<ExitStatus, BoundaryError> {
    loop {
        if let Some(status) = child.try_wait().map_err(|_| BoundaryError::Io)? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            converge_child(child);
            return Err(BoundaryError::Deadline);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn converge_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
