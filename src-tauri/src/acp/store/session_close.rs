use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::ProjectionRevision,
    lifecycle::{SessionState, TurnState},
};

use super::{
    MutationDisposition, StoreError,
    facts::{entity_revision_from_i64, next_projection_revision},
    sessions::{
        CloseSessionCommand, SessionMutation, append_intent_fact, ensure_one, update_chat,
        validate_chat_revision,
    },
    terminalize::terminalize_epoch,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalizeCloseCommand {
    pub request: CloseSessionCommand,
}

/// It deliberately does not claim that the process or session is closed.
pub(crate) fn close_session(
    connection: &mut rusqlite::Connection,
    command: CloseSessionCommand,
) -> Result<SessionMutation, StoreError> {
    let transaction = connection.transaction()?;
    let (state,revision,projection):(String,i64,i64)=transaction.query_row("SELECT state,revision,projection_revision FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3",params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?.ok_or(StoreError::Conflict{resource:"session"})?;
    if matches!(state.as_str(), "closing" | "closed") {
        if !close_intent_matches(&transaction, &command)? {
            return Err(StoreError::Conflict {
                resource: "clientIntent",
            });
        }
        return Ok(SessionMutation {
            session_id: command.session_id,
            runtime_epoch: Some(command.runtime_epoch),
            state: if state == "closing" {
                SessionState::Closing
            } else {
                SessionState::Closed
            },
            revision: entity_revision_from_i64(revision)?,
            projection_revision: ProjectionRevision::new(projection as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            disposition: MutationDisposition::Duplicate,
        });
    }
    if matches!(state.as_str(), "failed" | "incompatible") {
        return Err(StoreError::TerminalConflict {
            resource: "session",
        });
    }
    validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    let epoch_state: String = transaction
        .query_row(
            "SELECT state FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=?2",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "runtimeEpoch",
        })?;
    if !matches!(epoch_state.as_str(), "starting" | "running" | "terminating") {
        return Err(StoreError::Conflict {
            resource: "runtimeEpoch",
        });
    }
    let next_projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    if epoch_state != "terminating" {
        ensure_one(transaction.execute("UPDATE session_runtime_epochs SET state='terminating' WHERE session_id=?1 AND runtime_epoch=?2",params![command.session_id.as_str(),command.runtime_epoch.get() as i64])?,"runtime epoch")?;
    }
    let next = entity_revision_from_i64(revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(transaction.execute("UPDATE agent_sessions SET state='closing',residency='hot',revision=?2,projection_revision=?3,updated_at=?4 WHERE id=?1",params![command.session_id.as_str(),next.get() as i64,next_projection.get() as i64,command.now_millis])?,"session")?;
    append_intent_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        Some(command.runtime_epoch),
        "sessionCloseIntent",
        &command.client_intent_id,
        next_projection,
        command.now_millis,
    )?;
    update_chat(
        &transaction,
        &command.project_id,
        &command.chat_id,
        next_projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(SessionMutation {
        session_id: command.session_id,
        runtime_epoch: Some(command.runtime_epoch),
        state: SessionState::Closing,
        revision: next,
        projection_revision: next_projection,
        disposition: MutationDisposition::Applied,
    })
}

/// Called only by the process supervisor after it has observed zero surviving
/// descendants for the exact runtime epoch.
pub(crate) fn finalize_close_after_zero_process(
    connection: &mut rusqlite::Connection,
    command: FinalizeCloseCommand,
) -> Result<SessionMutation, StoreError> {
    let command = command.request;
    let transaction = connection.transaction()?;
    let (state,revision,projection):(String,i64,i64)=transaction.query_row("SELECT state,revision,projection_revision FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3",params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?.ok_or(StoreError::Conflict{resource:"session"})?;
    if !close_intent_matches(&transaction, &command)? {
        return Err(StoreError::Conflict {
            resource: "clientIntent",
        });
    }
    let epoch_state: String = transaction
        .query_row(
            "SELECT state FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=?2",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "runtimeEpoch",
        })?;
    if state == "closed" && epoch_state == "exited" {
        return Ok(SessionMutation {
            session_id: command.session_id,
            runtime_epoch: Some(command.runtime_epoch),
            state: SessionState::Closed,
            revision: entity_revision_from_i64(revision)?,
            projection_revision: ProjectionRevision::new(projection as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            disposition: MutationDisposition::Duplicate,
        });
    }
    if state != "closing" || epoch_state != "terminating" {
        return Err(StoreError::InvalidTransition {
            resource: "closeFinalize",
        });
    }
    let next_projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    terminalize_epoch(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        command.runtime_epoch,
        TurnState::Cancelled,
        "closeFinalized",
        next_projection,
        command.now_millis,
    )?;
    ensure_one(transaction.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=?3,exit_kind='userClose' WHERE session_id=?1 AND runtime_epoch=?2 AND state='terminating'",params![command.session_id.as_str(),command.runtime_epoch.get() as i64,command.now_millis])?,"runtime epoch")?;
    transaction.execute(
        "DELETE FROM chat_session_heads WHERE session_id=?1",
        [command.session_id.as_str()],
    )?;
    let next = entity_revision_from_i64(revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(transaction.execute("UPDATE agent_sessions SET state='closed',residency='cold',revision=?2,projection_revision=?3,updated_at=?4,terminal_at=?4 WHERE id=?1 AND state='closing'",params![command.session_id.as_str(),next.get() as i64,next_projection.get() as i64,command.now_millis])?,"session")?;
    update_chat(
        &transaction,
        &command.project_id,
        &command.chat_id,
        next_projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(SessionMutation {
        session_id: command.session_id,
        runtime_epoch: Some(command.runtime_epoch),
        state: SessionState::Closed,
        revision: next,
        projection_revision: next_projection,
        disposition: MutationDisposition::Applied,
    })
}

fn close_intent_matches(
    transaction: &Transaction<'_>,
    command: &CloseSessionCommand,
) -> Result<bool, StoreError> {
    let row:Option<i64>=transaction.query_row("SELECT json_extract(payload_json,'$.runtimeEpoch') FROM session_facts WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND kind='sessionCloseIntent' AND json_extract(payload_json,'$.clientIntentId')=?4 LIMIT 1",params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str(),command.client_intent_id.as_str()],|row|row.get(0)).optional()?;
    Ok(row == Some(command.runtime_epoch.get() as i64))
}
