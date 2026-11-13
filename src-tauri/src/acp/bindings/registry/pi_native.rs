//! Curated registry record for the production, read-only Pi binding .

use std::sync::Arc;

use crate::acp::{
    bindings::{
        pi::{
            PI_VERSION,
            native_admission::{PI_BINDING_ID, PiBindingRegistration},
            native_runtime::PiNativeRuntimeFactory,
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

pub(super) fn record(registration: Arc<PiBindingRegistration>) -> BindingRecord {
    BindingRecord {
        harness_id: "pi",
        binding_id: PI_BINDING_ID,
        wire: None,
        root_policy: RootPolicy::Pi,
        session_operations_enabled: true,
        admission: Admission::PiNative(registration),
        launch_factory,
        supported_capabilities: capabilities,
        turn_dispatch: crate::acp::service::AcpService::send_session_turn,
        cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
        close_dispatch: crate::acp::service::AcpService::close_session,
        model_provider: ModelProvider::NoAccountModels,
        auth_provider: AuthProvider::None,
    }
}

fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::PiNative(binding), BindingProjectRoot::Pi(root)) = (binding, root) else {
        unreachable!("Pi native authority mismatch")
    };
    Box::new(PiNativeRuntimeFactory { binding, root, mcp })
}

/// The truthful ceiling: prompt, cancel, steer, resume and Pi's own
/// compaction; no load, close, fork, or child projection.
fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::PiNative(binding) = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    binding
        .revalidate()
        .map_err(|_| RegistryAdmissionUnavailable::Drift)?;
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", format!("pi-rpc-{PI_VERSION}"))
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: true,
        load_session: false,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: true,
        native_session_compact: true,
        native_review: false,
        active_turn_steer: true,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        // Counts arrive from the native session handshake, not installation metadata.
        model_count: 0,
        prompt_resources: true,
        prompt_images: true,
        config_count: 0,
    })
}
