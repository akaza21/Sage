//! Claude Code over its Agent SDK protocol.
//!
//! Sage launches the installed `claude` CLI in SDK mode (stream-json with
//! control requests), the protocol both official Agent SDKs speak, so no
//! Node runtime or adapter process sits between Sage and Claude Code.

mod admission;
mod bridge;
mod config;
#[cfg(all(test, feature = "acp-fixture"))]
mod fixture_tests;
mod probe;
mod runtime;
mod tools;

pub(crate) use admission::{
    AdmittedClaudeSdkBinding, CLAUDE_SDK_BINDING_ID, ClaudeSdkBindingRegistration,
    launch_environment,
};
pub(crate) use probe::{known_models, models as probed_models, offers as offers_model};
pub(crate) use runtime::ClaudeSdkRuntimeFactory;
