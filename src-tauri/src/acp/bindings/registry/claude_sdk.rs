//! Curated registry record for Claude Code over its Agent SDK protocol.

use std::sync::Arc;

use crate::acp::{
    bindings::{
        claude::sdk::{
            CLAUDE_SDK_BINDING_ID, ClaudeSdkBindingRegistration, ClaudeSdkRuntimeFactory,
        },
        runtime::RuntimeFactory,
    },
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{
    Admission, AdmittedBinding, AuthProvider, BindingProjectRoot, BindingRecord, ModelProvider,
    RegistryAdmissionUnavailable, RootPolicy,
};

pub(super) fn record(registration: Arc<ClaudeSdkBindingRegistration>) -> BindingRecord {
    BindingRecord {
        harness_id: "claude-code",
        binding_id: CLAUDE_SDK_BINDING_ID,
        wire: None,
        root_policy: RootPolicy::Claude,
        session_operations_enabled: true,
        admission: Admission::ClaudeSdk(registration),
        launch_factory,
        supported_capabilities: capabilities,
        turn_dispatch: crate::acp::service::AcpService::send_session_turn,
        cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
        close_dispatch: crate::acp::service::AcpService::close_session,
        model_provider: ModelProvider::ClaudeAdapter,
        auth_provider: AuthProvider::Claude,
    }
}

/// A desktop fixture record: the replay case stands in for Claude Code and
/// no Claude sign-in is consulted.
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(super) fn fixture_record(
    selection: crate::launch_profile::FixtureSelection,
) -> Result<BindingRecord, crate::acp::bindings::adapter::AdapterRuntimeError> {
    let registration = ClaudeSdkBindingRegistration::desktop_fixture(selection)?;
    let mut record = record(Arc::new(registration));
    record.auth_provider = AuthProvider::None;
    Ok(record)
}

fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::ClaudeSdk(binding), BindingProjectRoot::Claude(root)) = (binding, root)
    else {
        unreachable!("Claude SDK record receives only Claude SDK admission and root authority")
    };
    // Sage's browser tools reach Claude Code over MCP in a later stage.
    drop(mcp);
    Box::new(ClaudeSdkRuntimeFactory { binding, root })
}

/// What the SDK protocol carries today: new and resumed sessions, prompts
/// with attachments, and cancel. Closing ends the process; fork and child
/// transcripts arrive with their own stages.
fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::ClaudeSdk(binding) = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    binding
        .revalidate()
        .map_err(|_| RegistryAdmissionUnavailable::Drift)?;
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "1")
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: true,
        load_session: false,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: false,
        native_review: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: true,
        prompt_images: false,
        config_count: 0,
    })
}
