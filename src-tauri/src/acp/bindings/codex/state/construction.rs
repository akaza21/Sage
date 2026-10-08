use super::{
    CodexAdapter, CommandItemSnapshots, SubagentLabels, TurnStartCorrelations,
    errors::observed_platform,
};
use crate::acp::bindings::codex::{
    admission::AdmittedCodexBinding, manifest::MetadataValidatedPacket,
};
use crate::acp::contract::{RuntimeEpoch, SageAgentSessionId};

impl CodexAdapter {
    #[cfg(test)]
    pub(crate) fn enable_observed_goal_for_test(&mut self) {
        self.runtime_version = "codex-cli 0.146.0".into();
        self.codex_home.clear();
        self.observed_build_characterisation =
            Some(crate::acp::bindings::characterisation::Characterisation::Characterised);
        if let Some(methods) = self.negotiated.as_mut() {
            methods.snapshot.active_turn_steer = true;
        }
    }

    pub(crate) fn from_metadata_packet(
        packet: &MetadataValidatedPacket,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            binding_digest,
            runtime_epoch,
            session_id,
            platform: packet.metadata().platform,
            codex_home: packet.codex_home().to_string_lossy().into_owned(),
            runtime_version: packet.metadata().codex_runtime_version.clone(),
            capability_ceiling: packet.capability_ceiling().clone(),
            observed_build_characterisation: None,
            pending: Vec::new(),
            negotiated: None,
            external_thread_id: None,
            browser_mcp_failure_reported: false,
            pending_browser_mcp_failure_threads: Vec::new(),
            turn_start_correlations: TurnStartCorrelations::new(),
            active_turn_id: None,
            provisional_turn: None,
            active_turn_client_message_id: None,
            active_item_terminals: Vec::new(),
            active_terminal_id_bytes: 0,
            previous_turn: None,
            command_item_snapshots: CommandItemSnapshots::default(),
            subagent_labels: SubagentLabels::default(),
            child_threads: Vec::new(),
            unknown_child_notification_reported: false,
            permission_requests: Vec::new(),
            goal_state: super::GoalState::Unsupported,
            goal_notification_generation: 0,
            goal_get_fence: None,
        }
    }

    pub(crate) fn from_admitted_binding(
        binding: &AdmittedCodexBinding,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        let mut adapter = Self::from_metadata_packet(
            binding.packet(),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        if binding.is_observed() {
            adapter.runtime_version = binding.harness_version().to_owned();
            adapter.codex_home.clear();
            adapter.platform = observed_platform();
            adapter.observed_build_characterisation = Some(binding.characterisation());
        }
        adapter
    }
}
