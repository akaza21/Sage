//! Codex-specific pre-launch authority proofs.
//!
//! These proofs do not admit or launch the Harness. They freeze the exact
//! configuration and cleared environment that a later native launcher must
//! consume.

#[path = "authority/config.rs"]
mod config;
#[path = "authority/environment.rs"]
mod environment;

pub(crate) use config::{ConfigAuthorityError, VerifiedCodexConfig, verify_codex_config};
pub(crate) use environment::{
    CodexLaunchEnvironment, EnvironmentAuthorityError, build_codex_launch_environment,
};
