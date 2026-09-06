use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};

use super::super::supervisor::LaunchError;

pub(super) fn observe_exit(handle: HANDLE) -> Result<(), LaunchError> {
    match unsafe { WaitForSingleObject(handle, INFINITE) } {
        WAIT_OBJECT_0 => Ok(()),
        _ => Err(LaunchError::ContainmentFailure),
    }
}

pub(super) fn try_observe_exit(handle: HANDLE) -> Result<bool, LaunchError> {
    match unsafe { WaitForSingleObject(handle, 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(LaunchError::ContainmentFailure),
    }
}

pub(super) fn wait_until(handle: HANDLE, deadline: Instant) -> Result<(), LaunchError> {
    wait_handle(handle, deadline.saturating_duration_since(Instant::now()))
}

fn wait_handle(handle: HANDLE, timeout: Duration) -> Result<(), LaunchError> {
    let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: handle is live and timeout is bounded.
    match unsafe { WaitForSingleObject(handle, milliseconds) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(LaunchError::Deadline),
        _ => Err(LaunchError::ContainmentFailure),
    }
}
