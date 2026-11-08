use crate::acp::{
    bindings::runtime::RuntimeFactory,
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{AdmittedBinding, BindingProjectRoot, RegistryAdmissionUnavailable};

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::Claude(binding), BindingProjectRoot::Claude(root)) = (binding, root)
    else {
        unreachable!("curated Claude record receives only Claude admission and root authority")
    };
    Box::new(crate::acp::bindings::claude::ClaudeRuntimeFactory::with_mcp(binding, root, mcp))
}

pub(super) fn capabilities(
    _binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "1")
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: true,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        // Characterised from the claude-agent-acp 0.76.0 / SDK 1.4.0 pair
        // (testdata/acp/claude/observed-0760-fork.json); the live offer and
        // the characterised-binding gate still intersect it.
        native_session_fork: true,
        native_session_compact: false,
        native_review: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: true,
        config_count: 0,
    })
}
