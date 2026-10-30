mod capability;
#[cfg(windows)]
mod cdp;
#[cfg(all(test, unix, feature = "cdp-lane"))]
mod cdp_lane;
#[cfg(any(windows, test))]
pub(crate) mod lanes;
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
#[cfg(windows)]
pub(crate) use native::pick_element;
#[cfg(windows)]
pub(crate) use native::set_annotate;
#[cfg(windows)]
mod picks;
#[cfg(windows)]
pub(crate) use picks::{unwatch_picks, watch_picks};
mod screenshot;
mod snapshot;
#[cfg(any(windows, test))]
#[cfg_attr(all(test, not(windows)), allow(dead_code))]
mod transport;
mod wait;

/// The native lane's own CDP observation of a tab, outside any Agent permit.
#[cfg(all(windows, feature = "native-lane"))]
pub(crate) fn lane_call(
    webview: &tauri::Webview,
    method: &str,
    params: &str,
    timeout: std::time::Duration,
) -> Result<String, crate::acp::browser::BrowserFailure> {
    cdp::call(webview, method, params, 1_048_576, timeout)
}

#[cfg(test)]
mod tests;
