use super::{
    AdapterBatchJournal, AdapterEvent, AdapterPump, DurableAdapterEvent, HarnessAdapter,
    ItemLifecycle, NormalizedFact, NormalizedPayload, ResourceMapper, TurnState,
};
use sage_acp_protocol::intent::SafeResourceLocation;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    /// A finished file change in the open Turn, with the bytes it left. Read
    /// before the change is journaled, so the record is of the agent's bytes.
    pub(in crate::acp::runtime::adapter_pump) fn agent_write(
        &mut self,
        event: &AdapterEvent,
        now_millis: i64,
        terminal: Option<TurnState>,
    ) -> Option<NormalizedFact> {
        let AdapterEvent::Item {
            id,
            lifecycle: ItemLifecycle::Completed,
            payload:
                NormalizedPayload::FileChangeDiff {
                    resource: SafeResourceLocation::Resource { resource },
                    ..
                },
            ..
        } = event
        else {
            return None;
        };
        if terminal.is_some() || !self.exact_turn(id) {
            return None;
        }
        let digest = self.resources.content_digest(resource)?;
        let bytes = resource.root_id.as_str().len() + resource.relative_path.as_str().len() + 32;
        Some(NormalizedFact::new(
            DurableAdapterEvent::AgentWrite {
                scope: self.scope(),
                resource: resource.clone(),
                digest,
                now_millis,
            },
            bytes,
        ))
    }
}
