//! Source-owned Codex App Server adapter packet.
//!
//! This module deliberately has no launcher. A validated candidate remains
//! unavailable until a separately admitted native artifact is supplied.

mod incoming;
mod manifest;
mod request;
mod runtime;
mod state;

pub(crate) use incoming::{IncomingEvent, IngressDisposition, ResourceMapper};
pub(crate) use manifest::{PhysicalAdmissionUnavailable, Platform, parse_packet_metadata};
pub(crate) use runtime::CodexRuntime;
pub(crate) use state::CodexAdapter;

#[cfg(test)]
mod tests;
