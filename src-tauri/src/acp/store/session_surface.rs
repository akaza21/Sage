//! Durable session-scoped state: one bounded row per agent session holding
//! the harness title, the newest notices, and the live epoch's model and
//! activity rows. Updates rewrite the row in place, so storage never grows
//! with the number of events.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::EntityRevision,
    intent::{
        ExternalId,
        session_event::{
            SessionNotice, SessionScopedEvent, SessionSurface, SessionSurfaceView, SurfaceChange,
        },
    },
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
    let projection = facts::next_projection_revision(transaction, project, chat)?;
    // The Turn's own record of an activity outlives this live surface.
    let recorded = match &command.event {
        SessionScopedEvent::Activity(row) => super::turn_activity_facts::record(
            transaction,
            owner,
            row,
            projection,
            command.now_millis,
        )?,
        _ => false,
    };
    let stored = read(transaction, project, chat, session)?;
    let revision = stored.as_ref().map(|(_, revision)| *revision);
    let mut surface = stored.map(|(surface, _)| surface).unwrap_or_default();
    let change = match surface.apply(owner.runtime_epoch.get(), &command.event) {
        Some(change) => change,
        None if recorded => {
            facts::publish_send_neutral_projection(
                transaction,
                project,
                chat,
                session,
                projection,
                command.now_millis,
            )?;
            return Ok(true);
        }
        None => return Ok(false),
    };
    match &command.event {
        SessionScopedEvent::Title { title: Some(title) } => {
            crate::chat_lifecycle::apply_harness_title_in(
                transaction,
                project,
                chat,
                title.as_str(),
                command.now_millis,
            )?
        }
        SessionScopedEvent::Title { title: None } => crate::chat_lifecycle::clear_harness_title_in(
            transaction,
            project,
            chat,
            command.now_millis,
        )?,
        _ => {}
    }
    let json = bounded_json(&mut surface)?;
    let next = match revision {
        Some(revision) => revision.next().map_err(|_| StoreError::CounterExhausted)?,
        None => EntityRevision::ZERO,
    };
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
    let publish = match change {
        SurfaceChange::SendNeutral => facts::publish_send_neutral_projection,
        SurfaceChange::SendRelevant => facts::publish_projection,
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

/// Serializes the surface within the row cap. Each field is byte-capped,
/// but JSON escaping can still push a full surface over it, so the oldest
/// display rows give way instead of the whole adapter batch failing.
fn bounded_json(surface: &mut SessionSurface) -> Result<String, StoreError> {
    loop {
        let json = serde_json::to_string(surface)
            .map_err(|_| StoreError::Corrupt("session surface serialization"))?;
        if json.len() <= SURFACE_BYTES_MAX {
            return Ok(json);
        }
        if !surface.evict_oldest() {
            return Err(StoreError::PayloadTooLarge);
        }
    }
}

pub(crate) fn view(
    connection: &rusqlite::Connection,
    project: &str,
    chat: &str,
    session: &str,
    runtime_epoch: Option<u64>,
) -> Result<Option<SessionSurfaceView>, StoreError> {
    Ok(read(connection, project, chat, session)?.map(|(surface, _)| surface.view(runtime_epoch)))
}

pub(crate) fn activity_id(
    connection: &rusqlite::Connection,
    project: &str,
    chat: &str,
    session: &str,
    key: &str,
) -> Result<Option<ExternalId>, StoreError> {
    Ok(read(connection, project, chat, session)?
        .and_then(|(surface, _)| surface.activity_id(key).cloned()))
}

pub(crate) fn is_undoable_turn(
    connection: &rusqlite::Connection,
    project: &str,
    chat: &str,
    session: &str,
    turn: &str,
) -> Result<bool, StoreError> {
    let latest: Option<(String, String, Option<i64>)> = connection
        .query_row(
            concat!(
                "SELECT t.id,t.session_id,t.terminal_at FROM turns t
                 WHERE t.project_id=?1 AND t.chat_id=?2 AND ",
                super::turn_undo::visible_turn!("t.session_id", "t.id"),
                " ORDER BY t.created_at DESC, t.ordinal DESC LIMIT 1"
            ),
            params![project, chat],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    Ok(latest
        .is_some_and(|(id, owner, terminal)| id == turn && owner == session && terminal.is_some()))
}

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

pub(super) fn notices(
    connection: &rusqlite::Connection,
    project: &str,
    chat: &str,
    session: Option<&str>,
) -> Result<Vec<SessionNotice>, StoreError> {
    let Some(session) = session else {
        return Ok(Vec::new());
    };
    Ok(read(connection, project, chat, session)?
        .map(|(surface, _)| surface.notices)
        .unwrap_or_default())
}
