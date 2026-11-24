//! Pi runtime construction stays separate from command dispatch.
use super::{PiRuntime, support};
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
use crate::acp::bindings::adapter::AdapterRuntimeError;
use crate::acp::bindings::pi::PiTranslator;
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId};
use std::collections::{BTreeMap, VecDeque};

impl PiRuntime {
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn from_fixture_binding(
        binding: &super::super::fixture_admission::PiFixtureBinding,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        binding.revalidate()?;
        Ok(Self::new(
            PiTranslator::fixture_admitted(runtime_epoch.get()),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        ))
    }

    pub(crate) fn from_native_binding(
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self::new(
            PiTranslator::native_admitted(runtime_epoch.get()),
            binding_digest,
            runtime_epoch,
            session_id,
        )
    }

    #[cfg(test)]
    pub(crate) fn fixture(runtime_epoch: RuntimeEpoch, session_id: SageAgentSessionId) -> Self {
        Self::new(
            PiTranslator::fixture_admitted(runtime_epoch.get()),
            [7; 32],
            runtime_epoch,
            session_id,
        )
    }

    fn new(
        translator: PiTranslator,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            bridge: None,
            translator,
            binding_digest,
            runtime_epoch,
            session_id,
            external_session: None,
            session_dir: None,
            session_file: None,
            active_turn: None,
            steering: support::Steering::default(),
            deny: None,
            cancelled_dialogs: BTreeMap::new(),
            recent_cancelled_dialogs: VecDeque::new(),
            questions: support::Questions::default(),
            staged_initialize: None,
            reservations: BTreeMap::new(),
            next_egress: 0,
            next_item: 0,
            model_count: 0,
            current_model: None,
            available_models: Vec::new(),
            current_thinking: None,
            available_thinking: Vec::new(),
            auto_compaction: None,
            commands: std::sync::Arc::default(),
            commands_pending: false,
            commands_discovered: false,
            configuration_dirty: false,
            model_request: None,
            model_disagreed: false,
            configuration_models_pending: false,
            thinking_request_session: None,
            thinking_request_owner: None,
            thinking_request_command: None,
            refresh_thinking_after_model: false,
            initialized: false,
            poisoned: false,
            native_pending: None,
            native_rpc_reply: None,
            usage: std::sync::Arc::default(),
            chrome: Default::default(),
            fork: None,
            forks: Vec::new(),
            launch: None,
            title_launch: None,
            title_request: None,
            title_result: None,
            #[cfg(test)]
            remove_file_for_test: |path| std::fs::remove_file(path),
        }
    }
}
