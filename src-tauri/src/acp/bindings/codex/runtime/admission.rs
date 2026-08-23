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
    pub(super) fn fixture(platform: super::super::manifest::Platform) -> Self {
        assert_eq!(platform, super::super::manifest::Platform::WindowsX86_64);
        let packet = super::super::manifest::parse_packet_metadata(include_bytes!(
            "../../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .expect("pinned Codex fixture");
        Self::new(
            &packet,
            [9; 32],
            RuntimeEpoch::new(1).expect("fixture epoch"),
            SageAgentSessionId::parse("session-codex-runtime").expect("fixture session"),
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
            inner: super::super::state::CodexAdapter::from_metadata_packet(
                packet,
                binding_digest,
                runtime_epoch,
                session_id,
            ),
            next_request_id: 0,
            snapshots: ItemSnapshots::new(),
            poisoned: false,
        }
    }

    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        let mut runtime = Self::new(
            binding.packet(),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        runtime.launch_authority = RuntimeLaunchAuthority::admitted(binding, project_root)?;
        Ok(runtime)
    }

    pub(crate) fn launch_environment(&self) -> Result<&[(String, String)], AdapterRuntimeError> {
        self.launch_authority
            .binding()
            .map(|binding| binding.environment().entries())
    }
}
