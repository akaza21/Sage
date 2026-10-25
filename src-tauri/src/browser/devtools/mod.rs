mod capability;
#[cfg(windows)]
mod cdp;
#[cfg(all(test, unix, feature = "cdp-lane"))]
mod cdp_lane;
#[cfg(all(feature = "cdp-lane", not(unix)))]
compile_error!("the cdp-lane feature runs headless Chromium and is unix-only");
// The command layer links into the app only on Windows; elsewhere it builds for
// its unit tests and the CDP lane.
#[cfg(any(windows, test))]
#[cfg_attr(all(test, not(windows)), allow(dead_code))]
mod cdp_port;
#[allow(dead_code)]
mod diagnostics;
mod events;
#[cfg(windows)]
pub(super) use events::detach;
#[cfg(windows)]
#[allow(dead_code)]
mod native;
#[cfg(windows)]
#[allow(unused_imports)]
pub(crate) use native::NativeDevtools;
#[cfg(windows)]
pub(crate) use native::capture_page_context;
#[cfg(windows)]
pub(crate) use native::capture_user_png;
#[cfg(windows)]
pub(crate) use native::clear_agent_highlight;
mod screenshot;
mod snapshot;
#[cfg(any(windows, test))]
#[cfg_attr(all(test, not(windows)), allow(dead_code))]
mod transport;
mod wait;

#[cfg(test)]
mod tests;
