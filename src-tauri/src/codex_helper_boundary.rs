#[path = "codex_helper_boundary/authority.rs"]
mod authority;
#[cfg(windows)]
#[path = "codex_helper_boundary/caller.rs"]
mod caller;
#[cfg(windows)]
#[path = "codex_helper_boundary/entrypoint.rs"]
mod entrypoint;
#[path = "codex_helper_boundary/executable.rs"]
mod executable;
#[path = "codex_helper_boundary/protocol.rs"]
mod protocol;
#[cfg(windows)]
#[path = "codex_helper_boundary/windows.rs"]
mod windows;

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
