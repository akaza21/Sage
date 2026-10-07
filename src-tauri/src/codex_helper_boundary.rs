#[cfg(any(windows, test))]
mod authority;
#[cfg(windows)]
mod caller;
#[cfg(any(windows, test))]
mod dispatch;
#[cfg(windows)]
mod entrypoint;
#[cfg(any(windows, test))]
mod executable;
#[cfg(any(windows, test))]
mod protocol;
#[cfg(windows)]
mod watchdog;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use authority::ElevatedBoundaryAuthority;
#[cfg(all(windows, test))]
pub(crate) use authority::{AuthorityObservation, PublicationAuthority, publication_authority};
#[cfg(windows)]
pub(crate) use dispatch::{PublicationExecutor, PublicationFailure, authenticated_response};
#[cfg(any(windows, test))]
pub use protocol::{BoundaryError, BoundaryStatus, HelperIdentity, OneShotSecret};

#[cfg(windows)]
pub use caller::{BoundaryProbeReceipt, direct_non_elevated_probe};

#[cfg(windows)]
pub fn run_helper_process(arguments: impl Iterator<Item = std::ffi::OsString>) -> i32 {
    match entrypoint::run(arguments) {
        Ok(()) => 0,
        Err(error) => match error {
            BoundaryError::Malformed => 10,
            BoundaryError::Bound => 11,
            BoundaryError::Authentication => 12,
            BoundaryError::Replay => 13,
            BoundaryError::HelperIdentity => 14,
            BoundaryError::Authority => 15,
            BoundaryError::Deadline => 16,
            BoundaryError::Io => 17,
        },
    }
}
