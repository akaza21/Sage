use crate::acp::{
    bindings::{
        claude::{AdmittedClaudeBinding, CLAUDE_BINDING_ID},
        runtime::RuntimeFactory,
    },
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{AdmittedBinding, BindingProjectRoot, RegistryAdmissionUnavailable};

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::Claude(binding), BindingProjectRoot::Claude(root)) = (binding, root)
    else {
        unreachable!("curated Claude record receives only Claude admission and root authority")
    };
    Box::new(
        crate::acp::bindings::claude::ClaudeRuntimeFactory::from_admitted_binding(binding, root),
    )
}

pub(super) fn capabilities(
    _binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    // ACP `initialize` is the authority for Claude capabilities. Before that
    // response the fixed wire ceiling admits only the core new-session surface.
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "1")
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: false,
        prompt: false,
        cancel: false,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    })
}

pub(super) fn binding_id(binding: &AdmittedClaudeBinding) -> &'static str {
    debug_assert_eq!(binding.binding_id(), CLAUDE_BINDING_ID);
    CLAUDE_BINDING_ID
}
