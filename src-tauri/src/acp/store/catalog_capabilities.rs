//! A live session republishes the capabilities it derives from its command
//! catalog. Only the running epoch of a ready or active session may do so.

use rusqlite::{OptionalExtension, params};
use sage_acp_protocol::{
    contract::ProjectionRevision, intent::CapabilitySnapshotV1, scheduler::RuntimeOwner,
};

use super::{StoreError, facts};

/// Stores `capabilities` for `owner` unless they are already stored. Returns
/// the projection revision of an applied change, or `None` when nothing
/// changed. A session not yet ready, or no longer run by `owner`, conflicts.
pub(crate) fn refresh_catalog_capabilities(
    connection: &mut rusqlite::Connection,
    owner: &RuntimeOwner,
    capabilities: &CapabilitySnapshotV1,
    now_millis: i64,
) -> Result<Option<ProjectionRevision>, StoreError> {
    let json = serde_json::to_string(capabilities)
        .map_err(|_| StoreError::Corrupt("catalog capabilities"))?;
    if json.len() > sage_core::limits::LIMITS.wire.capability_metadata_bytes as usize {
        return Err(StoreError::PayloadTooLarge);
    }
    let (project, chat, session) = (
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
    );
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let stored: String = transaction
        .query_row(
            "SELECT s.capabilities_json FROM agent_sessions s
             JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4
             AND s.state IN ('ready','active') AND e.state='running'",
            params![project, chat, session, owner.runtime_epoch.get() as i64],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "catalogCapabilities",
        })?;
    if stored == json {
        return Ok(None);
    }
    let projection = facts::next_projection_revision(&transaction, project, chat)?;
    super::sessions::ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET capabilities_json=?2,updated_at=?3 WHERE id=?1",
            params![session, json, now_millis],
        )?,
        "session",
    )?;
    // A racing send stays current: its admission reads the session afresh.
    facts::publish_send_neutral_projection(
        &transaction,
        project,
        chat,
        session,
        projection,
        now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(Some(projection))
}
