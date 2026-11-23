//! A live session republishes the capabilities it derives from its command
//! catalog. Only the running epoch of a ready or active session may do so.

use rusqlite::{OptionalExtension, params};
use sage_acp_protocol::{
    contract::ProjectionRevision, intent::CapabilitySnapshotV1, scheduler::RuntimeOwner,
};

use super::{StoreError, facts};

/// The binding's curated ceiling and Sage's policy; every stored capability
/// stays within both.
pub(crate) struct CapabilityBounds<'a> {
    pub(crate) curated: &'a CapabilitySnapshotV1,
    pub(crate) policy: &'a CapabilitySnapshotV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogRefresh {
    Applied(ProjectionRevision),
    Unchanged,
    /// The session is still starting; its activation commits first.
    NotReady,
}

/// The fields a command catalog may move. Every other negotiated field stays
/// as the activation stored it.
fn merge_catalog_fields(stored: &mut CapabilitySnapshotV1, clamped: &CapabilitySnapshotV1) {
    stored.goal = clamped.goal;
}

/// Writes the catalog-derived fields of `advertised`, clamped to `bounds`,
/// onto the snapshot stored for `owner`. A session no longer run by `owner`,
/// a snapshot outside `bounds` or an unreadable stored snapshot is refused.
pub(crate) fn refresh_catalog_capabilities(
    connection: &mut rusqlite::Connection,
    owner: &RuntimeOwner,
    bounds: CapabilityBounds<'_>,
    advertised: &CapabilitySnapshotV1,
    now_millis: i64,
) -> Result<CatalogRefresh, StoreError> {
    let clamped =
        crate::acp::runtime::intersect_capabilities(bounds.curated, advertised, bounds.policy)
            .map_err(|_| StoreError::Corrupt("catalog capabilities"))?;
    let (project, chat, session) = (
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
    );
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let (state, epoch, stored): (String, String, String) = transaction
        .query_row(
            "SELECT s.state,e.state,s.capabilities_json FROM agent_sessions s
             JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
            params![project, chat, session, owner.runtime_epoch.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "catalogCapabilities",
        })?;
    match (state.as_str(), epoch.as_str()) {
        ("starting", "starting" | "running") => return Ok(CatalogRefresh::NotReady),
        ("ready" | "active", "running") => {}
        _ => {
            return Err(StoreError::Conflict {
                resource: "catalogCapabilities",
            });
        }
    }
    let mut snapshot = serde_json::from_str::<CapabilitySnapshotV1>(&stored)
        .map_err(|_| StoreError::Corrupt("stored capabilities"))?;
    merge_catalog_fields(&mut snapshot, &clamped);
    let json = serde_json::to_string(&snapshot)
        .map_err(|_| StoreError::Corrupt("catalog capabilities"))?;
    if json == stored {
        return Ok(CatalogRefresh::Unchanged);
    }
    if json.len() > sage_core::limits::LIMITS.wire.capability_metadata_bytes as usize {
        return Err(StoreError::PayloadTooLarge);
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
    Ok(CatalogRefresh::Applied(projection))
}
