use std::ffi::OsString;
use std::time::{Duration, Instant};

use super::authority::AuthorityObservation;
use super::executable::VerifiedHelperExecutable;
use super::protocol::{
    BoundaryError, HelperIdentity, OneShotSecret, WIRE_REQUEST_BYTES_MAX, decode_request,
    split_wire_request,
};
use super::windows::{authority, channel};
use super::{
    ElevatedBoundaryAuthority, PublicationExecutor, PublicationFailure, authenticated_response,
};

const PROTOCOL_ARGUMENT: &str = "--sage-codex-helper-boundary-v1";
const PROCESS_DEADLINE: Duration = Duration::from_secs(2);

struct CorePublicationExecutor;

impl PublicationExecutor for CorePublicationExecutor {
    fn publish(
        &mut self,
        authority: &ElevatedBoundaryAuthority,
        payload: &[u8],
        secret: &OneShotSecret,
        helper: HelperIdentity,
    ) -> Result<(), PublicationFailure> {
        crate::acp::bindings::codex::execute_boundary_publication(
            authority, payload, secret, helper,
        )
        .map(drop)
    }
}

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

    let image = VerifiedHelperExecutable::open(
        &std::env::current_exe().map_err(|_| BoundaryError::HelperIdentity)?,
    )?;
    let (client_pid, client) = authority::connected_client(&pipe)?;
    let response = authenticated_response(
        &AuthorityObservation {
            expected_caller_pid: request.caller_pid,
            observed_caller_pid: client_pid,
            expected_caller_sid_sha256: request.caller_sid_sha256,
            observed_caller_sid_sha256: client.sid_sha256,
            caller_elevated: client.elevated,
            helper_elevated: helper_subject.elevated,
        },
        &request,
        &secret,
        image.identity(),
        &mut CorePublicationExecutor,
    )?;
    channel::write_message(&pipe.0, &response)
}

pub(crate) fn protocol_argument() -> &'static str {
    PROTOCOL_ARGUMENT
}
