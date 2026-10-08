mod capability;
#[cfg(windows)]
mod cdp;
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
mod wait;

#[cfg(test)]
mod tests;
