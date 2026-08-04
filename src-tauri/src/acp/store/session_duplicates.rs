use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{ProjectionRevision, SageAgentSessionId},
    lifecycle::SessionState,
};

use super::{
    MutationDisposition, StartDuplicateRow, StoreError,
    facts::entity_revision_from_i64,
    sessions::{ActivateSessionCommand, SessionMutation, StartSessionCommand},
};

pub(super) fn start_duplicate(
    transaction: &Transaction<'_>,
    command: &StartSessionCommand,
) -> Result<Option<SessionMutation>, StoreError> {
    let row: Option<StartDuplicateRow> = transaction
        .query_row(
            "SELECT s.id,s.binding_id,s.binding_digest,s.harness_version,s.model_id,s.state,
                    s.revision,s.projection_revision
             FROM session_facts f JOIN agent_sessions s ON s.id=f.session_id
             WHERE f.project_id=?1 AND f.chat_id=?2 AND f.kind='sessionStartIntent'
               AND json_extract(f.payload_json,'$.clientIntentId')=?3 LIMIT 1",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.client_intent_id.as_str()
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some(row) = row else { return Ok(None) };
    if row.1 != command.binding_id.as_str()
        || row.2 != command.binding_digest
        || row.3.as_deref() != command.harness_version.as_ref().map(|value| value.as_str())
        || row.4.as_deref() != command.model_id.as_ref().map(|value| value.as_str())
    {
        return Err(StoreError::Conflict {
            resource: "clientIntent",
        });
    }
    Ok(Some(SessionMutation {
        session_id: SageAgentSessionId::parse(row.0)
            .map_err(|_| StoreError::Corrupt("session id"))?,
        runtime_epoch: None,
        state: super::state::parse_session_state(&row.5)?,
        revision: entity_revision_from_i64(row.6)?,
        projection_revision: ProjectionRevision::new(row.7 as u64)
            .map_err(|_| StoreError::Corrupt("projection revision"))?,
        disposition: MutationDisposition::Duplicate,
    }))
}

pub(super) fn activation_duplicate(
    transaction: &Transaction<'_>,
    command: &ActivateSessionCommand,
) -> Result<Option<SessionMutation>, StoreError> {
    let committed_epoch: Option<i64> = transaction
        .query_row(
            "SELECT json_extract(payload_json,'$.runtimeEpoch') FROM session_facts
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3
               AND kind='sessionActivateIntent'
               AND json_extract(payload_json,'$.clientIntentId')=?4 LIMIT 1",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.client_intent_id.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    let Some(committed_epoch) = committed_epoch else {
        return Ok(None);
    };
    if committed_epoch != command.runtime_epoch.get() as i64 {
        return Err(StoreError::Conflict {
            resource: "clientIntent",
        });
    }
    let artifact_digest: String = transaction.query_row(
        "SELECT artifact_digest FROM session_runtime_epochs WHERE session_id=?1
         AND runtime_epoch=?2",
        params![
            command.session_id.as_str(),
            command.runtime_epoch.get() as i64
        ],
        |row| row.get(0),
    )?;
    if artifact_digest != command.artifact_digest {
        return Err(StoreError::Conflict {
            resource: "activationArtifact",
        });
    }
    let (state, revision, projection): (String, i64, i64) = transaction.query_row(
        "SELECT state,revision,projection_revision FROM agent_sessions WHERE id=?1",
        [command.session_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(Some(SessionMutation {
        session_id: command.session_id.clone(),
        runtime_epoch: Some(command.runtime_epoch),
        state: parse_duplicate_state(&state)?,
        revision: entity_revision_from_i64(revision)?,
        projection_revision: ProjectionRevision::new(projection as u64)
            .map_err(|_| StoreError::Corrupt("projection revision"))?,
        disposition: MutationDisposition::Duplicate,
    }))
}

fn parse_duplicate_state(value: &str) -> Result<SessionState, StoreError> {
    super::state::parse_session_state(value)
}
