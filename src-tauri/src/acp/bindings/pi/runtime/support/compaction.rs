use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterRuntimeError},
    intent::NormalizedPayload,
    lifecycle::ItemLifecycle,
};

use super::super::PiRuntime;

impl PiRuntime {
    pub(super) fn start_compaction(&mut self) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = self.next_local_item("compaction")?;
        self.active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?
            .compaction_id = Some(id.clone());
        self.item(
            id,
            ItemLifecycle::Started,
            None,
            NormalizedPayload::CompactionNotice,
        )
    }

    pub(super) fn finish_compaction(&mut self) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = self
            .active_turn
            .as_mut()
            .and_then(|turn| turn.compaction_id.take())
            .ok_or(AdapterRuntimeError::Operation)?;
        self.item(
            id,
            ItemLifecycle::Completed,
            None,
            NormalizedPayload::CompactionNotice,
        )
    }
}
