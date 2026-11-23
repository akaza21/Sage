use crate::acp::{
    runtime::adapter_pump::{DurableAdapterEvent, PumpIntent, PumpJournalError},
    store::AdapterInboundEvent,
};

use super::{scope_heap_bytes, verify_scope};

pub(super) fn event_bytes(
    intent: &PumpIntent,
    event: &DurableAdapterEvent,
) -> Result<usize, PumpJournalError> {
    let DurableAdapterEvent::AgentWrite {
        scope, resource, ..
    } = event
    else {
        return Err(PumpJournalError::Rejected);
    };
    verify_scope(intent, scope)?;
    Ok(scope_heap_bytes(scope)
        + resource.root_id.as_str().len()
        + resource.relative_path.as_str().len()
        + 32)
}

pub(super) fn convert(event: DurableAdapterEvent) -> Result<AdapterInboundEvent, PumpJournalError> {
    let DurableAdapterEvent::AgentWrite {
        resource,
        digest,
        now_millis,
        ..
    } = event
    else {
        return Err(PumpJournalError::Rejected);
    };
    Ok(AdapterInboundEvent::AgentWrite {
        resource,
        digest,
        now_millis,
    })
}
