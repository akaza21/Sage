use super::{
    CodexAdapter, CommandItemSnapshots, SubagentLabels, TurnStartCorrelations,
    errors::observed_platform,
};
use crate::acp::bindings::codex::{
    admission::AdmittedCodexBinding, manifest::MetadataValidatedPacket,
};
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId};

impl CodexAdapter {
    #[cfg(test)]
    pub(crate) fn enable_observed_goal_for_test(&mut self) {
        self.observe_build_for_test(
            &format!(
                "codex-cli {}",
                sage_acp_protocol::codex_protocol::PINNED_CODEX_VERSION
            ),
            crate::acp::bindings::characterisation::Characterisation::Characterised,
        );
        if let Some(methods) = self.negotiated.as_mut() {
            methods.snapshot.active_turn_steer = true;
            // An observed build reads the user's own Codex home.
            methods.snapshot.native_session_import = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn observe_build_for_test(
        &mut self,
        version_line: &str,
        characterisation: crate::acp::bindings::characterisation::Characterisation,
    ) {
        self.runtime_version = version_line.to_owned();
        self.codex_home.clear();
        self.observed_build_characterisation = Some(characterisation);
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
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            desktop_fixture: false,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            desktop_fixture_lists_models: false,
            pending: Vec::new(),
            negotiated: None,
            external_thread_id: None,
            browser_mcp_failure_reported: false,
            pending_browser_mcp_failure_threads: Vec::new(),
            pending_session_notices: Vec::new(),
            recent_warnings: Vec::new(),
            turn_start_correlations: TurnStartCorrelations::new(),
            active_turn_id: None,
            provisional_turn: None,
            action_handoff: None,
            deferred_interrupt: false,
            active_turn_client_message_id: None,
            active_item_terminals: Vec::new(),
            active_terminal_id_bytes: 0,
            active_unretried_error_fingerprints: Vec::new(),
            active_approval_reviews: Vec::new(),
            active_turn_user_stop: false,
            active_turn_plan: None,
            previous_turn: None,
            command_item_snapshots: CommandItemSnapshots::default(),
            running_mcp_tools: Default::default(),
            running_hooks: Default::default(),
            compactions: Default::default(),
            subagent_labels: SubagentLabels::default(),
            child_threads: Vec::new(),
            bound_child_threads: Vec::new(),
            unknown_child_notification_reported: false,
            permission_requests: Vec::new(),
            goal_state: super::GoalState::Unsupported,
            goal_notification_generation: 0,
            goal_get_fence: None,
            goal_mutation_fence: None,
            pending_read: None,
            child_read_tombstone: None,
            abandoned_reads: Vec::new(),
            native_root: None,
            listed_sessions: Vec::new(),
            rate_limits_abandoned: super::rate_limits::AbandonedReads::default(),
            terminals_tombstones: Vec::new(),
            mcp_read_tombstones: Vec::new(),
            skills: Default::default(),
            pending_fork: None,
            forks: super::fork::ForkLedger::default(),
            revert_target: None,
            reverted_turn: None,
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
        if binding.is_observed() || binding.is_desktop_fixture() {
            adapter.runtime_version = binding.harness_version().to_owned();
            adapter.codex_home.clear();
            adapter.platform = observed_platform();
            adapter.observed_build_characterisation = Some(binding.characterisation());
        }
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        {
            adapter.desktop_fixture = binding.is_desktop_fixture();
            adapter.desktop_fixture_lists_models = binding
                .desktop_fixture_case()
                .is_some_and(sage_acp_protocol::native_cases::native_case_lists_models);
        }
        adapter
    }
}
