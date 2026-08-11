use std::ffi::OsString;
use std::time::{Duration, Instant};

use super::authority::{AuthorityObservation, boundary_status};
use super::executable::VerifiedHelperExecutable;
use super::protocol::{
    BoundaryError, WIRE_REQUEST_BYTES_MAX, decode_request, seal_response, split_wire_request,
};
use super::windows::{authority, channel};

const PROTOCOL_ARGUMENT: &str = "--sage-codex-helper-boundary-v1";
const PROCESS_DEADLINE: Duration = Duration::from_secs(2);

pub(crate) fn run(mut arguments: impl Iterator<Item = OsString>) -> Result<(), BoundaryError> {
    let protocol = arguments.next().ok_or(BoundaryError::Malformed)?;
    let pipe_name = arguments.next().ok_or(BoundaryError::Malformed)?;
    if protocol != PROTOCOL_ARGUMENT || arguments.next().is_some() {
        return Err(BoundaryError::Malformed);
    }
    let pipe_name = pipe_name
        .into_string()
        .map_err(|_| BoundaryError::Malformed)?;
    channel::validate_name(&pipe_name)?;
    let helper_subject = authority::current_process_subject()?;
    let pipe = channel::create_server(&pipe_name, &helper_subject.sid)?;
    let deadline = Instant::now() + PROCESS_DEADLINE;
    channel::accept(&pipe, deadline)?;
    let wire = channel::read_message(&pipe.0, WIRE_REQUEST_BYTES_MAX, deadline)?;
    let (secret, sealed) = split_wire_request(&wire)?;
    let request = decode_request(sealed, &secret)?;
    let _opaque_request_bytes = request.payload.len();

    let image = VerifiedHelperExecutable::open(
        &std::env::current_exe().map_err(|_| BoundaryError::HelperIdentity)?,
    )?;
    if image.identity() != request.helper {
        return Err(BoundaryError::HelperIdentity);
    }
    let (client_pid, client) = authority::connected_client(&pipe)?;
    let status = boundary_status(&AuthorityObservation {
        expected_caller_pid: request.caller_pid,
        observed_caller_pid: client_pid,
        expected_caller_sid_sha256: request.caller_sid_sha256,
        observed_caller_sid_sha256: client.sid_sha256,
        caller_elevated: client.elevated,
        helper_elevated: helper_subject.elevated,
    })?;
    let response = seal_response(status, &request, &secret);
    channel::write_message(&pipe.0, &response)
}

pub(crate) fn protocol_argument() -> &'static str {
    PROTOCOL_ARGUMENT
}
