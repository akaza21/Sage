use rusqlite::Transaction;

use super::{AdapterInboundEvent, AdapterStoreOwner, StoreError};
use crate::acp::intent::NormalizedPayload;

/// A pair's pending metadata is never published on its own. Both events must
/// be adjacent and owner/key matched in the same already-bounded batch.
pub(super) fn apply_events(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    events: &[AdapterInboundEvent],
) -> Result<bool, StoreError> {
    let mut events = events.iter();
    let mut applied = false;
    while let Some(event) = events.next() {
        if let AdapterInboundEvent::Item(item) = event
            && matches!(
                item.payload,
                NormalizedPayload::FileChangeDiff {
                    pair_available: true,
                    ..
                }
            )
        {
            let Some(AdapterInboundEvent::FilePair(pair)) = events.next() else {
                return Err(StoreError::BatchShape);
            };
            super::verify_item_scope(owner, item)?;
            super::verify_file_pair_scope(owner, pair)?;
            if item.external_item_id != pair.external_item_id {
                return Err(StoreError::Conflict {
                    resource: "filePairItem",
                });
            }
            applied |= super::super::snapshots::apply_pair_in(transaction, item, pair)?;
        } else {
            applied |= super::apply_event(transaction, owner, event)?;
        }
    }
    Ok(applied)
}
