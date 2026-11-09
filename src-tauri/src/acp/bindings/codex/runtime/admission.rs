use std::sync::Arc;

use super::{
    CodexRuntime, ItemSnapshots, RuntimeLaunchAuthority, authority::CodexProjectRootAuthority,
};
use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{
            admission::AdmittedCodexBinding, managed::ManagedStagingPrerequisite,
            manifest::MetadataValidatedPacket, policy::SecurityPolicyPrerequisite,
            reproduction::ReproducedPackagePrerequisite,
        },
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    declared_test::declared_test_commands_for_project,
};

impl CodexRuntime {
    pub(crate) fn from_metadata_packet(
        packet: &MetadataValidatedPacket,
        security_policy: &SecurityPolicyPrerequisite,
        reproduced_package: &ReproducedPackagePrerequisite,
        managed_staging: &ManagedStagingPrerequisite,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        reproduced_package
            .revalidate(packet)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        managed_staging
            .revalidate(packet, reproduced_package)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        security_policy
            .revalidate(packet)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        packet
            .require_physical_admission()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self::new(packet, binding_digest, runtime_epoch, session_id))
    }

    #[cfg(test)]
    pub(crate) fn fixture(platform: super::super::manifest::Platform) -> Self {
        Self::fixture_with_identity(
            platform,
            [9; 32],
            SageAgentSessionId::parse("session-codex-runtime").expect("fixture session"),
        )
    }

    #[cfg(test)]
    pub(crate) fn fixture_with_identity(
        platform: super::super::manifest::Platform,
        binding_digest: [u8; 32],
        session_id: SageAgentSessionId,
    ) -> Self {
        assert_eq!(platform, super::super::manifest::Platform::WindowsX86_64);
        let packet = super::super::manifest::parse_packet_metadata(include_bytes!(
            "../../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .expect("pinned Codex fixture");
        Self::new(
            &packet,
            binding_digest,
            RuntimeEpoch::new(1).expect("fixture epoch"),
            session_id,
        )
    }

    fn new(
        packet: &MetadataValidatedPacket,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            launch_authority: RuntimeLaunchAuthority::metadata_only(),
            mcp: Default::default(),
            declared_test_commands: Default::default(),
            inner: super::super::state::CodexAdapter::from_metadata_packet(
                packet,
                binding_digest,
                runtime_epoch,
                session_id,
            ),
            next_request_id: 0,
            snapshots: ItemSnapshots::new(),
            poisoned: false,
            pending_browser_failure_details: Vec::new(),
            browser_failure_warning_logged: false,
            native_rpc_reply: None,
            usage: std::sync::Arc::default(),
            rate_limits: Default::default(),
            selected_model_id: None,
            reported_model_id: None,
            selected_model: None,
            model_catalog: Vec::new(),
            selected_effort: None,
            model_list_requested: false,
            model_catalog_idle_only: false,
            model_catalog_entries: 0,
            model_catalog_bytes: 0,
            next_model_cursor: None,
            model_pages_seen: 0,
            session_config: None,
        }
    }

    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        Self::from_admitted_binding_with_mcp(
            binding,
            project_root,
            runtime_epoch,
            session_id,
            Default::default(),
        )
    }

    pub(crate) fn from_admitted_binding_with_mcp(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
        mcp: crate::acp::mcp::McpAdmission,
    ) -> Result<Self, AdapterRuntimeError> {
        let declared_test_commands = declared_test_commands_for_project(project_root.as_path());
        let mut runtime = Self {
            launch_authority: RuntimeLaunchAuthority::metadata_only(),
            mcp,
            declared_test_commands,
            inner: super::super::state::CodexAdapter::from_admitted_binding(
                &binding,
                runtime_epoch,
                session_id,
            ),
            next_request_id: 0,
            snapshots: ItemSnapshots::new(),
            poisoned: false,
            pending_browser_failure_details: Vec::new(),
            browser_failure_warning_logged: false,
            native_rpc_reply: None,
            usage: std::sync::Arc::default(),
            rate_limits: Default::default(),
            selected_model_id: None,
            reported_model_id: None,
            selected_model: None,
            model_catalog: Vec::new(),
            selected_effort: None,
            model_list_requested: false,
            model_catalog_idle_only: false,
            model_catalog_entries: 0,
            model_catalog_bytes: 0,
            next_model_cursor: None,
            model_pages_seen: 0,
            session_config: None,
        };
        runtime.launch_authority = RuntimeLaunchAuthority::admitted(binding, project_root)?;
        Ok(runtime)
    }

    pub(crate) fn launch_environment(&self) -> Result<&[(String, String)], AdapterRuntimeError> {
        self.launch_authority
            .binding()
            .map(|binding| binding.launch_environment())
    }
}
