//! The ACP vocabulary: types and pure functions that describe the agent
//! conversation, shared by settings, the IPC types, the store, the runtime
//! and the bindings. That covers the wire contract and its bounded text,
//! the normalized intent and payload types, server-request lifecycles,
//! runtime ownership, session configuration values, permissions, file diff
//! references, usage, page admission, command output and the Codex protocol
//! identities. Behind `fixture`, it also holds the native replay catalogue.
//!
//! Invariant: no I/O, no async and no SQLite here, and no other Sage crate
//! but `sage-core`. Code that needs them belongs in `sage-acp` or above.

pub mod access_mode;
pub mod codex_protocol;
pub mod command_output;
pub mod contract;
pub mod declared_test;
pub mod file_diff;
#[cfg(all(feature = "fixture", feature = "test-support"))]
pub mod fixture_protocol;
pub mod intent;
pub mod lifecycle;
#[cfg(feature = "fixture")]
pub mod native_cases;
pub mod page_admission;
pub mod permissions;
pub mod scheduler;
pub mod session_config;
pub mod session_control;
pub mod usage;
