use crate::acp::{
    intent::NormalizedPayload,
    runtime::adapter_pump::{DurableAdapterEvent, PumpIntent, PumpJournalError},
    store::{AdapterInboundEvent, ItemIngressCommand},
};

use super::{scope_heap_bytes, verify_payload, verify_scope};

pub(super) fn event_bytes(
    intent: &PumpIntent,
    event: &DurableAdapterEvent,
) -> Result<usize, PumpJournalError> {
    let DurableAdapterEvent::ChildThreadItem {
        scope,
        external_item_id,
        payload,
        payload_digest,
        child_thread_id,
        ..
    } = event
    else {
        return Err(PumpJournalError::Rejected);
    };
    verify_scope(intent, scope)?;
    if !matches!(payload, NormalizedPayload::Collaboration { .. })
        || child_thread_id == &intent.external_session_id
    {
        return Err(PumpJournalError::Rejected);
    }
    let payload_bytes = verify_payload(payload, payload_digest)?;
    Ok(scope_heap_bytes(scope)
        + external_item_id.as_str().len()
        + payload_digest.len()
        + payload_bytes
        + child_thread_id.as_str().len())
}

pub(super) fn convert(event: DurableAdapterEvent) -> AdapterInboundEvent {
    let DurableAdapterEvent::ChildThreadItem {
        scope,
        external_item_id,
        lifecycle,
        payload,
        payload_digest,
        child_thread_id,
        now_millis,
    } = event
    else {
        unreachable!("child-thread event routed by caller")
    };
    AdapterInboundEvent::ChildThreadItem {
        item: ItemIngressCommand {
            project_id: scope.project_id,
            chat_id: scope.chat_id,
            session_id: scope.session_id,
            turn_id: scope.turn_id,
            runtime_epoch: scope.runtime_epoch,
            external_item_id,
            activity_id: None,
            payload,
            payload_digest,
            lifecycle,
            tool_state: None,
            now_millis,
        },
        child_thread_id,
    }
}

pub(super) fn live_event_bytes(
    intent: &PumpIntent,
    event: &DurableAdapterEvent,
) -> Result<usize, PumpJournalError> {
    let DurableAdapterEvent::ChildLiveItem {
        scope,
        child_thread_id,
        child_turn_id,
        item_id,
        role,
        payload,
        payload_digest,
        ..
    } = event
    else {
        return Err(PumpJournalError::Rejected);
    };
    verify_scope(intent, scope)?;
    if child_thread_id == &intent.external_session_id
        || !match payload {
            NormalizedPayload::Text { .. } => role.is_some(),
            NormalizedPayload::ToolCall { .. }
            | NormalizedPayload::FileChangeDiff { .. }
            | NormalizedPayload::ThoughtSummary { .. } => role.is_none(),
            _ => false,
        }
    {
        return Err(PumpJournalError::Rejected);
    }
    let payload_bytes = verify_payload(payload, payload_digest)?;
    Ok(scope_heap_bytes(scope)
        + child_thread_id.as_str().len()
        + child_turn_id.as_str().len()
        + item_id.as_str().len()
        + payload_digest.len()
        + payload_bytes)
}

pub(super) fn convert_live(event: DurableAdapterEvent) -> AdapterInboundEvent {
    let DurableAdapterEvent::ChildLiveItem {
        child_thread_id,
        child_turn_id,
        item_id,
        role,
        lifecycle,
        tool_state,
        payload,
        now_millis,
        ..
    } = event
    else {
        unreachable!("child-live event routed by caller")
    };
    AdapterInboundEvent::ChildLiveItem(crate::acp::store::ChildLiveItemCommand {
        child_thread_id,
        child_turn_id,
        item_id,
        role,
        lifecycle,
        tool_state,
        payload,
        now_millis,
    })
}
