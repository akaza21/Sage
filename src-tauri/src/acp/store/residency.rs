use rusqlite::{OptionalExtension, params};

use sage_acp_protocol::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    lifecycle::SessionState,
};

use super::{
    MutationDisposition, StoreError,
    facts::{append_fact_payload, entity_revision_from_i64, next_projection_revision},
    sessions::{SessionMutation, ensure_one, update_chat},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParkSessionCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub exit_kind: RequiredText<128>,
    pub now_millis: i64,
}

/// Records a proven zero-process boundary without closing the durable session.
/// A subsequent activation receives a new runtime epoch and exactly one
/// advertised resume/load attempt; this transition never replays a prompt.
pub(crate) fn park_session_after_zero_process(
    connection: &mut rusqlite::Connection,
    command: ParkSessionCommand,
) -> Result<SessionMutation, StoreError> {
    let transaction = connection.transaction()?;
    let row: (String, String, i64, i64, Option<String>) = transaction
        .query_row(
            "SELECT s.state,e.state,s.revision,s.projection_revision,e.exit_kind
             FROM agent_sessions s JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
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
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "runtimeEpoch",
        })?;

    if row.0 == "ready" && row.1 == "exited" {
        if row.4.as_deref() != Some(command.exit_kind.as_str()) {
            return Err(StoreError::TerminalConflict {
                resource: "runtimeEpoch",
            });
        }
        return Ok(SessionMutation {
            session_id: command.session_id,
            runtime_epoch: Some(command.runtime_epoch),
            state: SessionState::Ready,
            revision: entity_revision_from_i64(row.2)?,
            projection_revision: sage_acp_protocol::contract::ProjectionRevision::new(
                u64::try_from(row.3).map_err(|_| StoreError::Corrupt("projection revision"))?,
            )
            .map_err(|_| StoreError::Corrupt("projection revision"))?,
            disposition: MutationDisposition::Duplicate,
        });
    }
    if !matches!(row.0.as_str(), "ready" | "active") || row.1 != "running" {
        return Err(StoreError::InvalidTransition {
            resource: "sessionPark",
        });
    }

    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE session_runtime_epochs SET state='exited',terminal_at=?3,exit_kind=?4
             WHERE session_id=?1 AND runtime_epoch=?2 AND state='running'",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64,
                command.now_millis,
                command.exit_kind.as_str()
            ],
        )?,
        "runtime epoch",
    )?;
    let revision = entity_revision_from_i64(row.2)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='ready',residency='cold',revision=?2,
             projection_revision=?3,updated_at=?4 WHERE id=?1",
            params![
                command.session_id.as_str(),
                revision.get() as i64,
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "session",
    )?;
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "sessionParked",
        "entityId": command.session_id.as_str(),
        "state": "cold",
        "runtimeEpoch": command.runtime_epoch.get()
    }))
    .map_err(|_| StoreError::Corrupt("park serialization"))?;
    append_fact_payload(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        None,
        "sessionParked",
        &payload,
        projection,
        command.now_millis,
    )?;
    update_chat(
        &transaction,
        &command.project_id,
        &command.chat_id,
        projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(SessionMutation {
        session_id: command.session_id,
        runtime_epoch: Some(command.runtime_epoch),
        state: SessionState::Ready,
        revision,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}
