use rusqlite::{OptionalExtension, Transaction, params};

use sage_acp_protocol::{
    intent::{ExternalId, NormalizedPayload},
    lifecycle::ItemLifecycle,
};

use super::{AdapterStoreOwner, MutationDisposition, StoreError};
use crate::acp::store::ItemIngressCommand;

/// An admitted parent Item and its native child identity commit together.
/// The display collaboration payload does not grant this authority. The
/// stored source tag predates other harnesses and stands for any harness's
/// subagent activity.
pub(super) fn apply(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    item: &ItemIngressCommand,
    child: &ExternalId,
) -> Result<bool, StoreError> {
    super::scope::verify_item_scope(owner, item)?;
    if !matches!(item.payload, NormalizedPayload::Collaboration { .. })
        || child == &owner.external_session_id
        || item.lifecycle == ItemLifecycle::Unsupported
    {
        return Err(StoreError::Conflict {
            resource: "childThreadItem",
        });
    }
    let mutation = super::super::item_ingress::apply_item_ingress_in(transaction, item)?;
    let kind: String = transaction.query_row(
        "SELECT kind FROM items WHERE id=?1 AND project_id=?2 AND chat_id=?3
         AND session_id=?4 AND turn_id=?5 AND runtime_epoch=?6",
        params![
            mutation.item_id.as_str(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            owner.turn_id.as_str(),
            owner.runtime_epoch.get() as i64,
        ],
        |row| row.get(0),
    )?;
    if kind != "collaboration" {
        return Err(StoreError::Conflict {
            resource: "childThreadItem",
        });
    }

    let existing_owner: Option<(String, String, String, i64, String)> = transaction
        .query_row(
            "SELECT project_id,chat_id,session_id,runtime_epoch,parent_thread_id
             FROM acp_child_thread_owners WHERE native_child_thread_id=?1",
            [child.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    match existing_owner {
        Some((project, chat, session, epoch, parent))
            if project == owner.project_id.as_str()
                && chat == owner.chat_id.as_str()
                && session == owner.session_id.as_str()
                && epoch == owner.runtime_epoch.get() as i64
                && parent == owner.external_session_id.as_str() => {}
        Some(_) => {
            return Err(StoreError::Conflict {
                resource: "childThreadOwner",
            });
        }
        None => {
            transaction.execute(
                "INSERT INTO acp_child_thread_owners
                 (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,
                  parent_thread_id,source,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,'codexSubAgentActivity',?7)",
                params![
                    child.as_str(),
                    owner.project_id.as_str(),
                    owner.chat_id.as_str(),
                    owner.session_id.as_str(),
                    owner.runtime_epoch.get() as i64,
                    owner.external_session_id.as_str(),
                    item.now_millis,
                ],
            )?;
        }
    }
    let existing_child: Option<String> = transaction
        .query_row(
            "SELECT native_child_thread_id FROM acp_child_thread_bindings WHERE item_id=?1",
            [mutation.item_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let inserted = match existing_child {
        Some(existing) if existing == child.as_str() => false,
        Some(_) => {
            return Err(StoreError::Conflict {
                resource: "childThreadItem",
            });
        }
        None => {
            transaction.execute(
                "INSERT INTO acp_child_thread_bindings
                 (item_id,project_id,chat_id,session_id,turn_id,runtime_epoch,
                  native_child_thread_id,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    mutation.item_id.as_str(),
                    owner.project_id.as_str(),
                    owner.chat_id.as_str(),
                    owner.session_id.as_str(),
                    owner.turn_id.as_str(),
                    owner.runtime_epoch.get() as i64,
                    child.as_str(),
                    item.now_millis,
                ],
            )?;
            true
        }
    };
    Ok(inserted || mutation.entity.disposition == MutationDisposition::Applied)
}
