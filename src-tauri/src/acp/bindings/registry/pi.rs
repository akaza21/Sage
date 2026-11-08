use std::sync::Arc;

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        pi::{
            PI_VERSION,
            fixture_admission::{PI_FIXTURE_BINDING_ID, PiFixtureBinding},
            fixture_runtime::PiFixtureRuntimeFactory,
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

pub(super) fn record(binding: Arc<PiFixtureBinding>) -> BindingRecord {
    BindingRecord {
        harness_id: "pi",
        binding_id: PI_FIXTURE_BINDING_ID,
        wire: None,
        root_policy: RootPolicy::Pi,
        session_operations_enabled: true,
        admission: Admission::Pi(binding),
        launch_factory,
        supported_capabilities: capabilities,
        turn_dispatch: crate::acp::service::AcpService::send_session_turn,
        cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
        close_dispatch: crate::acp::service::AcpService::close_session,
        model_provider: ModelProvider::NoAccountModels,
        auth_provider: AuthProvider::None,
    }
}

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::Pi(binding), BindingProjectRoot::Pi(root)) = (binding, root) else {
        unreachable!("Pi fixture authority mismatch")
    };
    Box::new(PiFixtureRuntimeFactory { binding, root, mcp })
}

pub(super) fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::Pi(binding) = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    binding
        .revalidate()
        .map_err(|_: AdapterRuntimeError| RegistryAdmissionUnavailable::Drift)?;
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", format!("pi-rpc-{PI_VERSION}"))
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: false,
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
        model_count: 2,
        prompt_resources: false,
        config_count: 2,
    })
}
