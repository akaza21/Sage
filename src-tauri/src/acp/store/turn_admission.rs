//! What a Turn must pass before it opens, whoever opens it: a user prompt
//! (`PrepareTurn`) or the harness itself (`OpenHarnessTurn`). Each check
//! runs inside the opening transaction on the store writer.

use rusqlite::{OptionalExtension, Transaction, params};
use sage_core::limits::LIMITS;

use super::{StoreError, facts::check_add};

/// The session's head runs `runtime_epoch`, is resident, and holds no Turn
/// the user undid, so a Turn may open on it.
pub(super) fn require_current_runtime(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    session_id: &str,
    runtime_epoch: sage_acp_protocol::contract::RuntimeEpoch,
) -> Result<(), StoreError> {
    let owner: Option<(String, String)> = transaction
        .query_row(
            "SELECT s.state,s.residency FROM agent_sessions s
         JOIN chat_session_heads h ON h.session_id=s.id
         JOIN session_runtime_epochs e ON e.session_id=s.id AND e.state='running'
         WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
            params![project_id, chat_id, session_id, runtime_epoch.get() as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match owner {
        Some((state, residency))
            if matches!(state.as_str(), "ready" | "active") && residency == "hot" => {}
        _ => {
            return Err(StoreError::Conflict {
                resource: "sessionEpoch",
            });
        }
    }
    // A runtime still holding a Turn the user undid is as stale as an
    // earlier epoch: its next Turn would answer with the undone Turn in
    // context. The undo's record and this check commit in SQLite's write
    // order, so a Turn either opens before the undo is recorded (which then
    // refuses it) or is refused here.
    match super::turn_undo::pending_thread_rewind(transaction, session_id)? {
        Some(rewind) if rewind.held_by(runtime_epoch) => Err(StoreError::Conflict {
            resource: "sessionEpoch",
        }),
        _ => Ok(()),
    }
}

/// A Turn never opens in a Project while Sage changes its repository's
/// working tree; the lease and this check share the store writer.
pub(super) fn ensure_repository_unleased(
    transaction: &Transaction<'_>,
    project_id: &str,
) -> Result<(), StoreError> {
    if crate::repository_mutation_guard::project_leased(transaction, project_id)? {
        return Err(StoreError::RepositoryBusy);
    }
    Ok(())
}

/// What opening a Turn adds to the durable history.
pub(super) struct TurnFootprint {
    pub(super) items: i64,
    pub(super) item_bytes: i64,
}

pub(super) fn ensure_turn_capacity(
    transaction: &Transaction<'_>,
    project_id: &str,
    session_id: &str,
    TurnFootprint { items, item_bytes }: TurnFootprint,
) -> Result<(), StoreError> {
    let session:(i64,i64,i64,i64)=transaction.query_row("SELECT turn_count,item_count,item_bytes,payload_bytes FROM acp_session_usage WHERE session_id=?1",[session_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?.ok_or(StoreError::Corrupt("missing session usage"))?;
    let project: (i64, i64) = transaction
        .query_row(
            "SELECT turn_count,item_count FROM acp_project_usage WHERE project_id=?1",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Corrupt("missing project usage"))?;
    let global:(i64,i64,i64,i64)=transaction.query_row("SELECT turn_count,item_count,item_bytes,shared_payload_bytes FROM acp_store_usage WHERE singleton=1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?.ok_or(StoreError::Corrupt("missing global usage"))?;
    check_add(
        session.0,
        1,
        LIMITS.durable.turns_per_session,
        "sessionTurns",
    )?;
    check_add(
        session.1,
        items,
        LIMITS.durable.items_per_session,
        "sessionItems",
    )?;
    check_add(
        session.2,
        item_bytes,
        LIMITS.durable.item_bytes_per_session,
        "sessionItemBytes",
    )?;
    check_add(
        session.3,
        item_bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "sessionPayload",
    )?;
    check_add(
        project.0,
        1,
        LIMITS.durable.turns_per_project,
        "projectTurns",
    )?;
    check_add(global.0, 1, LIMITS.durable.turns_global, "globalTurns")?;
    check_add(global.1, items, LIMITS.durable.items_global, "globalItems")?;
    check_add(
        global.2,
        item_bytes,
        LIMITS.durable.item_bytes_global,
        "globalItemBytes",
    )?;
    check_add(
        global.3,
        item_bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "globalPayload",
    )
}
