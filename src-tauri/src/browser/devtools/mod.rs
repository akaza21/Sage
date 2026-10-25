mod capability;
#[cfg(windows)]
mod cdp;
#[cfg(all(test, unix, feature = "cdp-lane"))]
mod cdp_lane;
// The command layer compiles everywhere so the CDP lane can test it; only the
// Windows WebView2 transport links it into the app.
#[cfg_attr(not(windows), allow(dead_code))]
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
#[cfg_attr(not(windows), allow(dead_code))]
mod transport;
mod wait;

#[cfg(test)]
mod tests;
