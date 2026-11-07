#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod xdg;
#[cfg(target_os = "macos")]
pub(super) use macos::dispatch;
#[cfg(windows)]
pub(super) use windows::{ActiveToast, dispatch};
#[cfg(target_os = "linux")]
pub(super) use xdg::dispatch;
