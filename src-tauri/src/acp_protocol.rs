//! The ACP vocabulary: types and pure functions that describe the agent
//! conversation, shared by settings, the IPC types, the store, the runtime
//! and the bindings.
//!
//! Invariant: no I/O, no async and no SQLite here. Code that needs them
//! belongs in `acp` or above. This module becomes the `sage-acp-protocol`
//! crate.

pub(crate) mod access_mode;
pub(crate) mod codex_protocol;
pub(crate) mod file_diff;
#[cfg(feature = "acp-fixture")]
pub(crate) mod native_cases;
pub(crate) mod permissions;
