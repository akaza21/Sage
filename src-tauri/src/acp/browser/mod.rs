pub(crate) mod attachment;
mod bounds;
mod contract;
pub mod developer;
mod fake;
pub mod mcp;
pub(crate) mod pause;
pub mod permission;
mod redact;
#[cfg(windows)]
pub(crate) mod session_capability;

pub(crate) use bounds::{admit_capture_pixels, admit_screenshot};
pub use contract::*;
pub use fake::FakeBrowserCapability;
pub(crate) use redact::{redact_console, redact_url, safe_page_text};

#[cfg(test)]
mod tests;
