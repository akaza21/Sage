pub(crate) mod discovery;
mod error;
mod health;
pub(crate) mod lifecycle;
mod lifecycle_active;
mod lifecycle_cleanup;
mod lifecycle_process;
mod lifecycle_registry;
mod lifecycle_slot;
mod lifecycle_start;
#[cfg(windows)]
mod lifecycle_windows;
mod origin;
mod projection;
mod url;

#[cfg(test)]
mod tests;

pub(crate) use discovery::{PreviewDiscoveryRequest, PreviewDiscoveryResponse, PreviewTargetRef};
pub(crate) use error::PreviewError;
pub(crate) use health::PreviewHealthResponse;
pub(crate) use lifecycle::{PreviewStartResponse, PreviewStopResponse};
pub(crate) use projection::{PreviewFailure, PreviewProjection, PreviewProjectionOwner};
