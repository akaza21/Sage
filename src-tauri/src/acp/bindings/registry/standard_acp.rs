//! A user-defined agent's close support is read from its own `initialize` advertisement.

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
use crate::settings_store::CustomAcpAgent;

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
        native_session_compact: false,
        native_session_import: false,
        native_review: false,
        native_background_terminals: false,
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
    pub(crate) fn custom_acp_registrations(
        &self,
    ) -> impl Iterator<Item = &Arc<CustomAcpRegistration>> {
        self.custom
            .iter()
            .filter_map(|record| match &record.admission {
                Admission::CustomAcp(registration) => Some(registration),
                _ => None,
            })
    }

    pub(crate) fn custom_acp(&self, id: &str) -> Option<&Arc<CustomAcpRegistration>> {
        self.custom_acp_registrations()
            .find(|registration| registration.serves(id))
    }

    pub(crate) fn apply_custom_acp_agents(&self, agents: &[CustomAcpAgent]) {
        let mut placed = vec![false; agents.len()];
        for registration in self.custom_acp_registrations() {
            let held = registration
                .agent_id()
                .and_then(|id| agents.iter().position(|agent| agent.id == id));
            if let Some(index) = held {
                placed[index] = true;
            }
            registration.apply(held.map(|index| &agents[index]));
        }
        for (agent, _) in agents.iter().zip(&placed).filter(|(_, placed)| !**placed) {
            if let Some(registration) = self
                .custom_acp_registrations()
                .find(|registration| registration.is_vacant())
            {
                registration.apply(Some(agent));
            }
        }
    }
}
