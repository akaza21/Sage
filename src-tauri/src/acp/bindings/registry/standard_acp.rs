//! Records for agents driven by the standard ACP runtime: OpenCode and the
//! user-defined agents in Settings. Both launch through one factory; only
//! the ceiling differs, since a user-defined agent's close support is read
//! from its own `initialize` advertisement.

use std::sync::Arc;

use crate::acp::{
    bindings::{
        registry::{AuthProvider, ModelProvider},
        runtime::RuntimeFactory,
        standard_acp::{CustomAcpRegistration, StandardAcpRuntimeFactory},
    },
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{
    Admission, AdmittedBinding, BindingProjectRoot, BindingRecord, BindingRegistry,
    RegistryAdmissionUnavailable, RootPolicy,
};
use crate::settings_store::{CustomAcpAgent, MAX_CUSTOM_ACP_AGENTS, custom_acp_slot};

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::StandardAcp { source, binding }, BindingProjectRoot::StandardAcp(root)) =
        (binding, root)
    else {
        unreachable!(
            "a standard ACP record receives only standard ACP admission and root authority"
        )
    };
    Box::new(StandardAcpRuntimeFactory::with_mcp(
        source, binding, root, mcp,
    ))
}

pub(super) fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::StandardAcp { .. } = binding else {
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

/// The record for one user-defined agent slot.
pub(super) fn custom_record(registration: Arc<CustomAcpRegistration>) -> BindingRecord {
    let profile = registration.profile();
    BindingRecord {
        harness_id: profile.harness_id,
        binding_id: profile.binding_id,
        wire: None,
        root_policy: RootPolicy::StandardAcp,
        session_operations_enabled: true,
        admission: Admission::CustomAcp(registration),
        launch_factory,
        supported_capabilities: capabilities,
        turn_dispatch: crate::acp::service::AcpService::send_session_turn,
        cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
        close_dispatch: crate::acp::service::AcpService::close_session,
        model_provider: ModelProvider::NoAccountModels,
        auth_provider: AuthProvider::None,
    }
}

impl BindingRegistry {
    /// The registration of user-defined agent `slot` (1-based).
    pub(crate) fn custom_acp(&self, slot: usize) -> Option<&Arc<CustomAcpRegistration>> {
        match &self.custom.get(slot.checked_sub(1)?)?.admission {
            Admission::CustomAcp(registration) => Some(registration),
            _ => None,
        }
    }

    /// Admits the saved user-defined agents; a slot with no agent is cleared.
    pub(crate) fn apply_custom_acp_agents(&self, agents: &[CustomAcpAgent]) {
        for slot in 1..=MAX_CUSTOM_ACP_AGENTS {
            if let Some(registration) = self.custom_acp(slot) {
                registration.apply(
                    agents
                        .iter()
                        .find(|agent| custom_acp_slot(&agent.id) == Some(slot)),
                );
            }
        }
    }
}
