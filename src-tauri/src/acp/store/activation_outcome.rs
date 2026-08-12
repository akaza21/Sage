use rusqlite::{OptionalExtension, params};

use crate::acp::{
    contract::{
        ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId,
    },
    intent::{CapabilitySnapshotV1, ExternalId},
    lifecycle::SessionState,
    limits::LIMITS,
};

use super::{
    MutationDisposition, StoreError,
    facts::{append_fact_payload, entity_revision_from_i64, next_projection_revision},
    sessions::{PENDING_CAPABILITIES_JSON, SessionMutation, ensure_one, update_chat, valid_digest},
    state::session_state_text,
};

mod abort;
pub(crate) use abort::{AbortActivationCommand, ActivationAbortCategory, abort_activation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompleteActivationCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub binding_digest: String,
    pub artifact_digest: String,
    pub external_session_id: ExternalId,
    pub capabilities_json: String,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailActivationCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub binding_digest: String,
    pub artifact_digest: String,
    pub terminal: SessionState,
    pub exit_kind: RequiredText<128>,
    pub now_millis: i64,
}

struct ActivationRow {
    session_state: String,
    external_session_id: Option<String>,
    capabilities_json: String,
    session_revision: i64,
    projection_revision: i64,
    binding_digest: String,
    epoch_state: String,
    artifact_digest: Option<String>,
    exit_kind: Option<String>,
    latest_epoch: i64,
}

pub(crate) fn complete_activation(
    connection: &mut rusqlite::Connection,
    command: CompleteActivationCommand,
) -> Result<SessionMutation, StoreError> {
    validate_completion(&command)?;
    let transaction = connection.transaction()?;
    let row = activation_row(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        command.runtime_epoch,
    )?;
    verify_digests(&row, &command.binding_digest, &command.artifact_digest)?;
    if row.epoch_state == "running" && matches!(row.session_state.as_str(), "ready" | "active") {
        if row.external_session_id.as_deref() == Some(command.external_session_id.as_str())
            && row.capabilities_json == command.capabilities_json
        {
            return duplicate(&command.session_id, command.runtime_epoch, &row);
        }
        return Err(StoreError::Conflict {
            resource: "activationResult",
        });
    }
    if row.epoch_state != "starting"
        || !matches!(row.session_state.as_str(), "starting" | "recovering")
    {
        return Err(StoreError::InvalidTransition {
            resource: "activationResult",
        });
    }
    let first_fill = row.session_state == "starting";
    if first_fill {
        if row.external_session_id.is_some() || row.capabilities_json != PENDING_CAPABILITIES_JSON {
            return Err(StoreError::Conflict {
                resource: "activationFill",
            });
        }
    } else if row.external_session_id.as_deref() != Some(command.external_session_id.as_str())
        || row.capabilities_json != command.capabilities_json
    {
        return Err(StoreError::Conflict {
            resource: "activationResume",
        });
    }
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE session_runtime_epochs SET state='running'
             WHERE session_id=?1 AND runtime_epoch=?2 AND state='starting'",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
        )?,
        "runtime epoch",
    )?;
    let next = entity_revision_from_i64(row.session_revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    if first_fill {
        ensure_one(
            transaction.execute(
                "UPDATE agent_sessions SET external_session_id=?2,capabilities_json=?3,
                 state='ready',residency='hot',revision=?4,projection_revision=?5,updated_at=?6
                 WHERE id=?1 AND state='starting'",
                params![
                    command.session_id.as_str(),
                    command.external_session_id.as_str(),
                    command.capabilities_json,
                    next.get() as i64,
                    projection.get() as i64,
                    command.now_millis
                ],
            )?,
            "session",
        )?;
    } else {
        ensure_one(
            transaction.execute(
                "UPDATE agent_sessions SET state='ready',residency='hot',revision=?2,
                 projection_revision=?3,updated_at=?4 WHERE id=?1 AND state='recovering'",
                params![
                    command.session_id.as_str(),
                    next.get() as i64,
                    projection.get() as i64,
                    command.now_millis
                ],
            )?,
            "session",
        )?;
    }
    append_activation_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        command.runtime_epoch,
        "activationReady",
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
        revision: next,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

pub(crate) fn fail_activation(
    connection: &mut rusqlite::Connection,
    command: FailActivationCommand,
) -> Result<SessionMutation, StoreError> {
    if !matches!(
        command.terminal,
        SessionState::Failed | SessionState::Incompatible
    ) || !valid_digest(&command.binding_digest)
        || !valid_digest(&command.artifact_digest)
    {
        return Err(StoreError::InvalidTransition {
            resource: "activationFailure",
        });
    }
    let transaction = connection.transaction()?;
    let row = activation_row(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        command.runtime_epoch,
    )?;
    verify_digests(&row, &command.binding_digest, &command.artifact_digest)?;
    if row.epoch_state == "failed" && parse_session(&row.session_state)? == command.terminal {
        if row.exit_kind.as_deref() == Some(command.exit_kind.as_str()) {
            return duplicate(&command.session_id, command.runtime_epoch, &row);
        }
        return Err(StoreError::TerminalConflict {
            resource: "activationFailure",
        });
    }
    if row.epoch_state != "starting"
        || !matches!(row.session_state.as_str(), "starting" | "recovering")
    {
        return Err(StoreError::InvalidTransition {
            resource: "activationFailure",
        });
    }
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE session_runtime_epochs SET state='failed',terminal_at=?3,exit_kind=?4
             WHERE session_id=?1 AND runtime_epoch=?2 AND state='starting'",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64,
                command.now_millis,
                command.exit_kind.as_str()
            ],
        )?,
        "runtime epoch",
    )?;
    transaction.execute(
        "DELETE FROM chat_session_heads WHERE session_id=?1",
        [command.session_id.as_str()],
    )?;
    append_activation_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        command.runtime_epoch,
        "activationFailed",
        projection,
        command.now_millis,
    )?;
    let next = entity_revision_from_i64(row.session_revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state=?2,residency='cold',revision=?3,
             projection_revision=?4,updated_at=?5,terminal_at=?5 WHERE id=?1",
            params![
                command.session_id.as_str(),
                session_state_text(command.terminal),
                next.get() as i64,
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "session",
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
        state: command.terminal,
        revision: next,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

fn validate_completion(command: &CompleteActivationCommand) -> Result<(), StoreError> {
    let capabilities = serde_json::from_str::<CapabilitySnapshotV1>(&command.capabilities_json)
        .map_err(|_| StoreError::Corrupt("activation result"))?;
    let canonical = serde_json::to_string(&capabilities)
        .map_err(|_| StoreError::Corrupt("activation result"))?;
    if !valid_digest(&command.binding_digest)
        || !valid_digest(&command.artifact_digest)
        || command.capabilities_json.len() > LIMITS.wire.capability_metadata_bytes as usize
        || capabilities.protocol_version.as_str() == "pending"
        || canonical != command.capabilities_json
    {
        return Err(StoreError::Corrupt("activation result"));
    }
    Ok(())
}
fn activation_row(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
) -> Result<ActivationRow, StoreError> {
    connection
        .query_row(
            "SELECT s.state,s.external_session_id,s.capabilities_json,s.revision,
                    s.projection_revision,s.binding_digest,e.state,e.artifact_digest,e.exit_kind,
                    (SELECT MAX(latest.runtime_epoch) FROM session_runtime_epochs latest
                     WHERE latest.session_id=s.id)
             FROM agent_sessions s JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
            params![
                project.as_str(),
                chat.as_str(),
                session.as_str(),
                epoch.get() as i64
            ],
            |row| {
                Ok(ActivationRow {
                    session_state: row.get(0)?,
                    external_session_id: row.get(1)?,
                    capabilities_json: row.get(2)?,
                    session_revision: row.get(3)?,
                    projection_revision: row.get(4)?,
                    binding_digest: row.get(5)?,
                    epoch_state: row.get(6)?,
                    artifact_digest: row.get(7)?,
                    exit_kind: row.get(8)?,
                    latest_epoch: row.get(9)?,
                })
            },
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "activationOwner",
        })
}
fn verify_digests(row: &ActivationRow, binding: &str, artifact: &str) -> Result<(), StoreError> {
    if row.binding_digest != binding || row.artifact_digest.as_deref() != Some(artifact) {
        return Err(StoreError::Conflict {
            resource: "activationDigest",
        });
    }
    Ok(())
}
fn duplicate(
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    row: &ActivationRow,
) -> Result<SessionMutation, StoreError> {
    Ok(SessionMutation {
        session_id: session.clone(),
        runtime_epoch: Some(epoch),
        state: parse_session(&row.session_state)?,
        revision: entity_revision_from_i64(row.session_revision)?,
        projection_revision: ProjectionRevision::new(row.projection_revision as u64)
            .map_err(|_| StoreError::Corrupt("projection revision"))?,
        disposition: MutationDisposition::Duplicate,
    })
}
fn parse_session(value: &str) -> Result<SessionState, StoreError> {
    super::state::parse_session_state(value)
}
#[allow(clippy::too_many_arguments)]
fn append_activation_fact(
    transaction: &rusqlite::Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    kind: &str,
    projection: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "entityId": session.as_str(),
        "state": "committed",
        "runtimeEpoch": epoch.get()
    }))
    .map_err(|_| StoreError::Corrupt("activation fact"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        None,
        kind,
        &payload,
        projection,
        now,
    )
}
