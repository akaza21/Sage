//! Claude Code over its Agent SDK protocol.
//!
//! Sage launches the installed `claude` CLI in SDK mode (stream-json with
//! control requests), the protocol both official Agent SDKs speak, so no
//! Node runtime or adapter process sits between Sage and Claude Code.

mod admission;
mod bridge;
mod config;
mod runtime;
mod tools;

pub(crate) use admission::{
    AdmittedClaudeSdkBinding, CLAUDE_SDK_BINDING_ID, ClaudeSdkBindingRegistration,
    launch_environment,
};
pub(crate) use runtime::ClaudeSdkRuntimeFactory;
