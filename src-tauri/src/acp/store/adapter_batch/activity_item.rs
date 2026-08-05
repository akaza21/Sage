use rusqlite::{Transaction, params};

use crate::acp::{
    contract::{SageActivityId, SageItemId},
    intent::ExternalId,
};

use super::{AdapterInboundEvent, AdapterStoreOwner, MutationDisposition, StoreError};

pub(super) fn apply(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    event: &AdapterInboundEvent,
) -> Result<bool, StoreError> {
    let AdapterInboundEvent::ActivityItem(command) = event else {
        unreachable!("activity item module accepts only activity items")
    };
    let super::AdapterActivityItemCommand {
        external_activity_id,
        activity_kind,
        parent_external_activity_id,
        parent_external_item_id,
        role,
        activity_lifecycle,
        depth,
        observed_sequence,
        provenance,
        provenance_digest,
        item,
    } = command.as_ref();
    super::verify_item_scope(owner, item)?;
    if item.activity_id.is_some() {
        return Err(StoreError::Conflict {
            resource: "adapterActivityItem",
        });
    }
    let parent_item_id = resolve_parent_item(transaction, owner, parent_external_item_id)?;
    let parent_activity_id = parent_external_activity_id
        .as_ref()
        .map(|external| resolve_parent_activity(transaction, owner, external))
        .transpose()?;
    let activity = super::super::activity_ingress::apply_activity_ingress_in(
        transaction,
        super::super::ActivityIngressCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            turn_id: owner.turn_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            external_activity_id: external_activity_id.clone(),
            activity_kind: activity_kind.clone(),
            parent_activity_id,
            parent_item_id,
            role: *role,
            lifecycle: *activity_lifecycle,
            depth: *depth,
            observed_sequence: *observed_sequence,
            provenance: provenance.clone(),
            provenance_digest: provenance_digest.clone(),
            now_millis: item.now_millis,
        },
    )?;
    let mut child = item.clone();
    child.activity_id = Some(activity.activity_id);
    let item = super::super::item_ingress::apply_item_ingress_in(transaction, &child)?;
    Ok(activity.entity.disposition == MutationDisposition::Applied
        || item.entity.disposition == MutationDisposition::Applied)
}

fn resolve_parent_item(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    external: &ExternalId,
) -> Result<SageItemId, StoreError> {
    let mut statement = transaction.prepare(
        "SELECT id FROM items WHERE project_id=?1 AND chat_id=?2 AND session_id=?3
         AND turn_id=?4 AND runtime_epoch=?5 AND external_item_id=?6 LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            owner.turn_id.as_str(),
            owner.runtime_epoch.get() as i64,
            external.as_str()
        ],
        |row| row.get::<_, String>(0),
    )?;
    let values = rows.collect::<Result<Vec<_>, _>>()?;
    let [value] = values.as_slice() else {
        return Err(StoreError::Conflict {
            resource: "activityParentItem",
        });
    };
    SageItemId::parse(value).map_err(|_| StoreError::Corrupt("activity parent item id"))
}

fn resolve_parent_activity(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    external: &ExternalId,
) -> Result<SageActivityId, StoreError> {
    let mut statement = transaction.prepare(
        "SELECT id FROM session_activities
         WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
           AND runtime_epoch=?5 AND external_activity_id=?6 LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            owner.turn_id.as_str(),
            owner.runtime_epoch.get() as i64,
            external.as_str()
        ],
        |row| row.get::<_, String>(0),
    )?;
    let values = rows.collect::<Result<Vec<_>, _>>()?;
    let [value] = values.as_slice() else {
        return Err(StoreError::Conflict {
            resource: "activityParentActivity",
        });
    };
    SageActivityId::parse(value).map_err(|_| StoreError::Corrupt("activity parent activity id"))
}
