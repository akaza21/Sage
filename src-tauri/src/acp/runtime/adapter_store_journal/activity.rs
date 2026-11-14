use crate::acp::{
    runtime::adapter_pump::{DurableActivityItem, PumpIntent, PumpJournalError},
    store::{AdapterActivityItemCommand, AdapterInboundEvent, ItemIngressCommand},
};
use sage_core::limits::LIMITS;

pub(super) fn event_bytes(
    intent: &PumpIntent,
    activity: &DurableActivityItem,
) -> Result<usize, PumpJournalError> {
    super::command::verify_scope(intent, &activity.scope)?;
    activity
        .provenance
        .validate_for_persistence()
        .map_err(|_| PumpJournalError::Rejected)?;
    let provenance_json =
        serde_json::to_string(&activity.provenance).map_err(|_| PumpJournalError::Rejected)?;
    if provenance_json.len() > LIMITS.wire.metadata_bytes as usize {
        return Err(PumpJournalError::Rejected);
    }
    let payload_bytes =
        super::command::verify_payload(&activity.payload, &activity.payload_digest)?;
    std::mem::size_of::<AdapterActivityItemCommand>()
        .checked_add(super::command::scope_heap_bytes(&activity.scope))
        .and_then(|bytes| bytes.checked_add(activity.external_activity_id.as_str().len()))
        .and_then(|bytes| bytes.checked_add(activity.external_item_id.as_str().len()))
        .and_then(|bytes| {
            bytes.checked_add(
                activity
                    .parent_external_activity_id
                    .as_ref()
                    .map_or(0, |id| id.as_str().len()),
            )
        })
        .and_then(|bytes| bytes.checked_add(activity.parent_external_item_id.as_str().len()))
        .and_then(|bytes| bytes.checked_add(activity.activity_kind.as_str().len()))
        .and_then(|bytes| bytes.checked_add(provenance_json.len()))
        .and_then(|bytes| bytes.checked_add(activity.payload_digest.len()))
        .and_then(|bytes| bytes.checked_add(payload_bytes))
        .ok_or(PumpJournalError::Rejected)
}

pub(super) fn convert(
    activity: DurableActivityItem,
) -> Result<AdapterInboundEvent, PumpJournalError> {
    let provenance_digest = super::command::digest(
        serde_json::to_string(&activity.provenance)
            .map_err(|_| PumpJournalError::Rejected)?
            .as_bytes(),
    );
    Ok(AdapterInboundEvent::ActivityItem(Box::new(
        AdapterActivityItemCommand {
            external_activity_id: activity.external_activity_id,
            activity_kind: activity.activity_kind,
            parent_external_activity_id: activity.parent_external_activity_id,
            parent_external_item_id: activity.parent_external_item_id,
            role: activity.role,
            activity_lifecycle: activity.activity_lifecycle,
            depth: activity.depth,
            observed_sequence: activity.observed_sequence,
            provenance: activity.provenance,
            provenance_digest,
            item: ItemIngressCommand {
                project_id: activity.scope.project_id,
                chat_id: activity.scope.chat_id,
                session_id: activity.scope.session_id,
                turn_id: activity.scope.turn_id,
                runtime_epoch: activity.scope.runtime_epoch,
                external_item_id: activity.external_item_id,
                activity_id: None,
                lifecycle: activity.item_lifecycle,
                tool_state: activity.item_tool_state,
                payload: activity.payload,
                payload_digest: activity.payload_digest,
                now_millis: activity.now_millis,
            },
        },
    )))
}
