//! Codex-specific pre-launch authority proofs.
//!
//! These proofs do not admit or launch the Harness. They freeze the exact
//! configuration and cleared environment that a later native launcher must
//! consume.

#[path = "authority/config.rs"]
mod config;
#[path = "authority/environment.rs"]
mod environment;
#[path = "authority/retention.rs"]
mod retention;
#[path = "authority/storage.rs"]
mod storage;

pub(crate) use config::{ConfigAuthorityError, VerifiedCodexConfig, verify_codex_config};
pub(crate) use environment::{
    CodexLaunchEnvironment, EnvironmentAuthorityError, build_codex_launch_environment,
};
pub(crate) use retention::{
    ResidueMarker, RetentionDecision, RetentionError, classify_codex_state_path,
};
pub(crate) use storage::{
    CodexAuthorityPrerequisite, StateAuthorityError, inspect_codex_authority,
};

#[cfg(all(test, windows))]
pub(crate) use storage::inject_before_state_recheck;
