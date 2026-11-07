use crate::acp::{
    bindings::{runtime::RuntimeFactory, standard_acp::OpenCodeRuntimeFactory},
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{AdmittedBinding, BindingProjectRoot, RegistryAdmissionUnavailable};

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (
        AdmittedBinding::OpenCode {
            registration,
            binding,
        },
        BindingProjectRoot::OpenCode(root),
    ) = (binding, root)
    else {
        unreachable!("curated OpenCode record receives only OpenCode admission and root authority")
    };
    Box::new(OpenCodeRuntimeFactory::with_mcp(
        registration,
        binding,
        root,
        mcp,
    ))
}

pub(super) fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::OpenCode { .. } = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "1")
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
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
