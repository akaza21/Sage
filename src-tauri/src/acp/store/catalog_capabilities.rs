//! A catalog change replaces the live capability snapshot under the same epoch authority.
use super::{StoreError, facts};
use rusqlite::{Connection, params};
use sage_acp_protocol::{intent::CapabilitySnapshotV1, scheduler::RuntimeOwner};

pub(crate) fn update(
    connection: &mut Connection,
    owner: &RuntimeOwner,
    capabilities: &CapabilitySnapshotV1,
) -> Result<Option<crate::acp::ipc::ProjectionChangedV1>, StoreError> {
    let json = serde_json::to_string(capabilities)
        .map_err(|_| StoreError::Corrupt("catalog capabilities"))?;
    if json.len() > sage_core::limits::LIMITS.wire.capability_metadata_bytes as usize {
        return Err(StoreError::Corrupt("catalog capabilities exceed bound"));
    }
    let transaction = connection.transaction()?;
    let revision = facts::next_projection_revision(
        &transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
    )?;
    let changed = transaction.execute(
        "UPDATE agent_sessions SET capabilities_json=?5,revision=revision+1,
             projection_revision=?6,updated_at=?7
         WHERE project_id=?1 AND chat_id=?2 AND id=?3 AND capabilities_json<>?5
         AND state IN ('ready','active') AND residency='hot'
         AND EXISTS(SELECT 1 FROM session_runtime_epochs e WHERE e.session_id=agent_sessions.id
             AND e.runtime_epoch=?4 AND e.state='running')",
        params![
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            owner.runtime_epoch.get() as i64,
            json,
            revision.get() as i64,
            sage_core::time::now_millis()
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    facts::publish_chat_projection(
        &transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        revision,
        sage_core::time::now_millis(),
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(Some(crate::acp::ipc::ProjectionChangedV1 {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        projection_revision: revision,
        child_projection_epoch: 0,
    }))
}
