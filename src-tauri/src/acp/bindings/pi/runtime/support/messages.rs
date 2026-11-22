use super::{AdapterEgress, AdapterEvent, AdapterRuntimeError, PiRuntime, identity};
use crate::acp::bindings::pi::{Operation, StateSnapshot};
use sage_acp_protocol::{intent::NormalizedPayload, lifecycle::ItemLifecycle};

impl PiRuntime {
    pub(super) fn message_item(
        &mut self,
        payload: NormalizedPayload,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = self.next_local_item("message")?;
        self.item(id, ItemLifecycle::Completed, None, payload)
    }

    pub(super) fn configuration_reply(
        &mut self,
        state: StateSnapshot,
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        self.refresh_thinking_after_model = true;
        self.current_model = state.model.as_ref().map(identity::model_key).transpose()?;
        self.current_thinking = state.thinking_level;
        self.auto_compaction = state.auto_compaction;
        // Optional like the read that asked for it.
        Ok((
            Some(AdapterEvent::IgnoredDuplicate),
            self.begin_one(Operation::GetModels, super::super::RequestKind::Models)
                .ok(),
        ))
    }
}
