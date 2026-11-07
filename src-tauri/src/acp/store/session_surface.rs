//! Durable session-scoped state: one bounded row per agent session holding
//! the harness title, the newest notices, and the live epoch's model and
//! activity rows. Updates rewrite the row in place, so storage never grows
//! with the number of events.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::EntityRevision,
    intent::session_event::{SessionScopedEvent, SessionSurface},
};

use super::{AdapterStoreOwner, StoreError, facts};

/// Upper bound of one serialized surface; the schema enforces the same cap.
pub(super) const SURFACE_BYTES_MAX: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionEventCommand {
    pub event: SessionScopedEvent,
    pub now_millis: i64,
}

/// Folds one session-scoped event into the owner session's row. The caller
/// has already verified that the owner is the session's live runtime epoch.
pub(super) fn apply_in(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    command: &SessionEventCommand,
) -> Result<bool, StoreError> {
    let (project, chat, session) = (
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
    );
    let stored = read(transaction, project, chat, session)?;
    let revision = stored.as_ref().map(|(_, revision)| *revision);
    let mut surface = stored.map(|(surface, _)| surface).unwrap_or_default();
    if !surface.apply(owner.runtime_epoch.get(), &command.event) {
        return Ok(false);
    }
    let json = serde_json::to_string(&surface)
        .map_err(|_| StoreError::Corrupt("session surface serialization"))?;
    if json.len() > SURFACE_BYTES_MAX {
        return Err(StoreError::PayloadTooLarge);
    }
    let next = match revision {
        Some(revision) => revision.next().map_err(|_| StoreError::CounterExhausted)?,
        None => EntityRevision::ZERO,
    };
    let projection = facts::next_projection_revision(transaction, project, chat)?;
    transaction.execute(
        "INSERT INTO acp_session_surfaces
           (session_id,project_id,chat_id,surface_json,revision,updated_at)
         VALUES (?3,?1,?2,?4,?5,?6)
         ON CONFLICT(session_id) DO UPDATE SET
           surface_json=excluded.surface_json,revision=excluded.revision,
           updated_at=excluded.updated_at",
        params![
            project,
            chat,
            session,
            json,
            next.get() as i64,
            command.now_millis
        ],
    )?;
    let publish = if command.event.is_send_neutral() {
        facts::publish_send_neutral_projection
    } else {
        facts::publish_projection
    };
    publish(
        transaction,
        project,
        chat,
        session,
        projection,
        command.now_millis,
    )?;
    Ok(true)
}

/// Reads one session's stored surface and its row revision.
pub(super) fn read(
    connection: &rusqlite::Connection,
    project: &str,
    chat: &str,
    session: &str,
) -> Result<Option<(SessionSurface, EntityRevision)>, StoreError> {
    let Some((json, revision)) = connection
        .query_row(
            "SELECT surface_json,revision FROM acp_session_surfaces
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3",
            params![project, chat, session],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let surface = serde_json::from_str::<SessionSurface>(&json)
        .ok()
        .filter(SessionSurface::is_bounded)
        .ok_or(StoreError::Corrupt("session surface"))?;
    let revision = u64::try_from(revision)
        .ok()
        .and_then(|revision| EntityRevision::new(revision).ok())
        .ok_or(StoreError::Corrupt("session surface revision"))?;
    Ok(Some((surface, revision)))
}
