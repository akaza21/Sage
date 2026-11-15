use super::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

const SETTLEMENT_FACTS: i64 = 3;

// Drop sequence's INTEGER affinity so SQLite can seek the JSON expression
// index, as browser operation recovery does for its start receipt.
pub(super) const PENDING_SQL: &str =
    "SELECT s.sequence,s.session_id,s.turn_id,s.payload_json FROM session_facts s
     WHERE s.project_id=?1 AND s.chat_id=?2 AND s.kind='turnUndoStarted'
       AND NOT EXISTS(SELECT 1 FROM session_facts f WHERE f.session_id=s.session_id
         AND f.turn_id=s.turn_id AND f.kind='turnUndoSettled'
         AND json_extract(f.payload_json,'$.start')=+s.sequence)
     ORDER BY s.sequence LIMIT 1";

fn reservation_bytes(command: &TurnUndoneCommand) -> i64 {
    // Reserve exact identity escaping and worst-width sequence/count fields.
    let undone = super::undo_payload(command, UndoneThread::Reverted).len();
    let settled = serde_json::json!({"start":i64::MAX,"state":"rolledBack"})
        .to_string()
        .len();
    let recovery = serde_json::json!({"kind":"restartRecovery",
        "entityId":command.session_id.as_str(),"state":"sessionFailed",
        "turns":u64::MAX,"items":u64::MAX,"activities":u64::MAX,"requests":u64::MAX})
    .to_string()
    .len();
    (undone + settled + recovery) as i64
}

pub(crate) struct PendingUndo {
    pub sequence: i64,
    pub command: TurnUndoneCommand,
}

pub(crate) fn ensure_settled(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<(), StoreError> {
    if pending(connection, project, chat)?.is_some() {
        return Err(StoreError::Conflict {
            resource: "unsettledUndo",
        });
    }
    Ok(())
}

pub(crate) fn pending(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<Option<PendingUndo>, StoreError> {
    connection
        .query_row(PENDING_SQL, params![project, chat], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .optional()?
        .map(|(sequence, session, turn, json)| {
            let value: serde_json::Value =
                serde_json::from_str(&json).map_err(|_| StoreError::Corrupt("undo intent"))?;
            let epoch = value["runtimeEpoch"]
                .as_u64()
                .and_then(|v| RuntimeEpoch::new(v).ok())
                .ok_or(StoreError::Corrupt("undo intent epoch"))?;
            let harness_reverted = value["harnessReverted"]
                .as_bool()
                .ok_or(StoreError::Corrupt("undo intent mode"))?;
            let cut_at = match value.get("cutAt") {
                Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(id)) => Some(id.clone()),
                _ => return Err(StoreError::Corrupt("undo intent cut")),
            };
            Ok(PendingUndo {
                sequence,
                command: TurnUndoneCommand {
                    project_id: SageProjectId::parse(project)
                        .map_err(|_| StoreError::Corrupt("undo project"))?,
                    chat_id: SageChatId::parse(chat)
                        .map_err(|_| StoreError::Corrupt("undo chat"))?,
                    session_id: SageAgentSessionId::parse(session)
                        .map_err(|_| StoreError::Corrupt("undo session"))?,
                    turn_id: SageTurnId::parse(turn)
                        .map_err(|_| StoreError::Corrupt("undo turn"))?,
                    runtime_epoch: epoch,
                    harness_reverted,
                    cut_at,
                    now_millis: 0,
                },
            })
        })
        .transpose()
}

/// Commit the validated plan and reserve completion and recovery facts before native IO.
pub(crate) fn start(
    connection: &mut Connection,
    command: &TurnUndoneCommand,
) -> Result<(), StoreError> {
    let tx = connection.transaction()?;
    let (project, chat, session, turn) = (
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        command.turn_id.as_str(),
    );
    let running: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_runtime_epochs e JOIN chat_session_heads h
         ON h.session_id=e.session_id WHERE h.project_id=?1 AND h.chat_id=?2
         AND e.session_id=?3 AND e.runtime_epoch=?4 AND e.state='running')",
        params![project, chat, session, command.runtime_epoch.get() as i64],
        |row| row.get(0),
    )?;
    if !running
        || pending(&tx, project, chat)?.is_some()
        || !super::super::session_surface::is_undoable_turn(&tx, project, chat, session, turn)?
    {
        return Err(StoreError::Conflict {
            resource: "undoIntent",
        });
    }
    let revision = next_projection_revision(&tx, project, chat)?;
    let payload = serde_json::json!({"state":"started", "runtimeEpoch":command.runtime_epoch.get(),
        "harnessReverted":command.harness_reverted, "cutAt":command.cut_at})
    .to_string();
    append_fact_payload(
        &tx,
        project,
        chat,
        session,
        Some(turn),
        "turnUndoStarted",
        &payload,
        revision,
        command.now_millis,
    )?;
    // Charge completion and recovery writes now, as browser starts do.
    for index in 0..SETTLEMENT_FACTS {
        let bytes = if index == 0 {
            reservation_bytes(command)
        } else {
            0
        };
        super::super::facts::ensure_fact_capacity(&tx, session, bytes)?;
        adjust_reservation(&tx, command, 1, bytes)?;
    }
    publish_send_neutral_projection(&tx, project, chat, session, revision, command.now_millis)?;
    tx.commit().map_err(|_| StoreError::CommitUnknown)
}

pub(crate) fn rollback(
    connection: &mut Connection,
    command: &TurnUndoneCommand,
) -> Result<(), StoreError> {
    let tx = connection.transaction()?;
    rollback_in(&tx, command)?;
    tx.commit().map_err(|_| StoreError::CommitUnknown)
}

pub(super) fn rollback_in(
    tx: &Transaction<'_>,
    command: &TurnUndoneCommand,
) -> Result<(), StoreError> {
    let Some(mut intent) = pending(tx, command.project_id.as_str(), command.chat_id.as_str())?
    else {
        return Ok(());
    };
    intent.command.now_millis = command.now_millis;
    if intent.command != *command {
        return Err(StoreError::Conflict {
            resource: "undoIntent",
        });
    }
    release_reservation(tx, command)?;
    let revision =
        next_projection_revision(tx, command.project_id.as_str(), command.chat_id.as_str())?;
    append_settled(tx, &intent, "rolledBack", revision, command.now_millis)?;
    publish_send_neutral_projection(
        tx,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        revision,
        command.now_millis,
    )?;
    Ok(())
}

pub(super) fn release_reservation(
    tx: &Transaction<'_>,
    command: &TurnUndoneCommand,
) -> Result<(), StoreError> {
    adjust_reservation(tx, command, -SETTLEMENT_FACTS, -reservation_bytes(command))
}

fn adjust_reservation(
    tx: &Transaction<'_>,
    command: &TurnUndoneCommand,
    count: i64,
    bytes: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE acp_session_usage SET fact_count=fact_count+?2,fact_bytes=fact_bytes+?3,
        payload_bytes=payload_bytes+?3,revision=revision+1 WHERE session_id=?1",
        params![command.session_id.as_str(), count, bytes],
    )?;
    tx.execute(
        "UPDATE acp_project_usage SET fact_count=fact_count+?2,payload_bytes=payload_bytes+?3,
        revision=revision+1 WHERE project_id=?1",
        params![command.project_id.as_str(), count, bytes],
    )?;
    tx.execute(
        "UPDATE acp_store_usage SET fact_count=fact_count+?1,fact_bytes=fact_bytes+?2,
        shared_payload_bytes=shared_payload_bytes+?2,revision=revision+1 WHERE singleton=1",
        params![count, bytes],
    )?;
    Ok(())
}

pub(super) fn append_settled(
    tx: &Transaction<'_>,
    intent: &PendingUndo,
    state: &str,
    revision: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let command = &intent.command;
    let payload = serde_json::json!({"start":intent.sequence,"state":state}).to_string();
    append_fact_payload(
        tx,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        Some(command.turn_id.as_str()),
        "turnUndoSettled",
        &payload,
        revision,
        now,
    )
}
