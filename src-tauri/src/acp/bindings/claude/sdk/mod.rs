//! Sage launches the installed `claude` CLI in SDK mode (stream-json with
//! control requests), the protocol both official Agent SDKs speak, so no
//! Node runtime or adapter process sits between Sage and Claude Code.

mod admission;
mod bridge;
mod config;
#[cfg(all(test, feature = "acp-fixture"))]
mod fixture_tests;
mod fork;
mod launch;
mod probe;
mod probe_exit;
mod runtime;
mod tools;
mod transcript;
pub(crate) use transcript::{TranscriptError, resolve_undo};

pub(crate) use admission::{
    AdmittedClaudeSdkBinding, CLAUDE_SDK_BINDING_ID, ClaudeSdkBindingRegistration,
    launch_environment,
};
pub(crate) use probe::{
    known_commands as probed_commands, known_models, known_start_mode, launch_effort_option,
    listing_failed, models as probed_models,
};
pub(crate) use runtime::ClaudeSdkRuntimeFactory;
pub(crate) use runtime::native::rewind_capability;

#[cfg(test)]
mod undo_tests;
