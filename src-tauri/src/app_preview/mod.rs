pub(crate) mod discovery;
mod error;
mod health;
pub(crate) mod lifecycle;
#[cfg(windows)]
mod lifecycle_windows;
mod origin;
mod url;

#[cfg(test)]
mod tests;

pub(crate) use discovery::{PreviewDiscoveryRequest, PreviewDiscoveryResponse, PreviewTargetRef};
pub(crate) use error::PreviewError;
pub(crate) use health::PreviewHealthResponse;
pub(crate) use lifecycle::{PreviewStartResponse, PreviewStopResponse};
