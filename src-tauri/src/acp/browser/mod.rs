pub(crate) mod attachment;
mod bounds;
mod contract;
mod fake;
pub mod mcp;
pub mod permission;
mod redact;
#[cfg(windows)]
pub(crate) mod session_capability;

pub(crate) use bounds::admit_screenshot;
pub use contract::*;
pub use fake::FakeBrowserCapability;
pub(crate) use redact::{redact_console, redact_url};

#[cfg(test)]
mod tests;
