mod diagnostics;
mod protocol;
mod runner;
pub(crate) mod schema;
mod transport;

pub(crate) use protocol::{CodexDiscoveryProtocol, DiscoveryRequest};
pub(crate) use runner::{CodexDiscoveryDeadline, CodexDiscoveryRunError, DiscoveryZeroProcess};
#[cfg(test)]
pub(crate) use runner::{DiscoveryTransport, RevalidationCheckpoint, run_discovery_protocol};
pub(crate) use transport::{
    AdmittedDiscovery, CodexProcessTransport, ProcessLaunchPolicy, discover_admitted,
};
pub(crate) use types::{
    CodexAccountSnapshot, CodexDiscoveryContent, CodexDiscoveryError, CodexModelSnapshot,
    ProtocolProgress,
};
mod types;

#[cfg(test)]
mod tests;
