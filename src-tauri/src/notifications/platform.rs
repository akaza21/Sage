#[cfg(not(windows))]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
pub(super) use unix::dispatch;
#[cfg(windows)]
pub(super) use windows::{ActiveToast, dispatch};
