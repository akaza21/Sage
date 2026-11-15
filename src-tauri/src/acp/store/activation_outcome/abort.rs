use rusqlite::params;

use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    lifecycle::SessionState,
};

use super::{
    MutationDisposition, SessionMutation, StoreError, activation_row, append_fact_payload,
    duplicate, ensure_one, entity_revision_from_i64, next_projection_revision, update_chat,
    valid_digest, verify_digests,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AbortActivationCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub binding_digest: String,
    pub artifact_digest: String,
    pub category: ActivationAbortCategory,
    pub now_millis: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Durable wire categories are frozen.
pub(crate) enum ActivationAbortCategory {
    LaunchFailure,
    AssemblyFailure,
    RuntimeHandoffFailure,
}

impl ActivationAbortCategory {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LaunchFailure => "launchFailure",
            Self::AssemblyFailure => "assemblyFailure",
            Self::RuntimeHandoffFailure => "runtimeHandoffFailure",
        }
    }

    #[cfg(test)]
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "launchFailure" => Some(Self::LaunchFailure),
            "assemblyFailure" => Some(Self::AssemblyFailure),
            "runtimeHandoffFailure" => Some(Self::RuntimeHandoffFailure),
            _ => None,
        }
    }
}

pub(crate) fn abort_activation(
    connection: &mut rusqlite::Connection,
    command: AbortActivationCommand,
) -> Result<SessionMutation, StoreError> {
    if !valid_digest(&command.binding_digest) || !valid_digest(&command.artifact_digest) {
        return Err(StoreError::InvalidTransition {
            resource: "activationAbort",
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
    if row.latest_epoch != command.runtime_epoch.get() as i64 {
        return Err(StoreError::InvalidTransition {
            resource: "activationAbort",
        });
    }
    let restored = match (
        row.session_state.as_str(),
        row.external_session_id.as_deref(),
    ) {
        ("starting", None) => "created",
        ("recovering", Some(_)) => "ready",
        ("created", None) | ("ready", Some(_)) if row.epoch_state == "aborted" => {
            if row.exit_kind.as_deref() != Some(command.category.as_str()) {
                return Err(StoreError::TerminalConflict {
                    resource: "activationAbort",
                });
            }
            return duplicate(&command.session_id, command.runtime_epoch, &row);
        }
        _ => {
            return Err(StoreError::InvalidTransition {
                resource: "activationAbort",
            });
        }
    };
    if row.epoch_state != "starting" {
        return Err(StoreError::InvalidTransition {
            resource: "activationAbort",
        });
    }
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE session_runtime_epochs SET state='aborted',terminal_at=?3,exit_kind=?4
         WHERE session_id=?1 AND runtime_epoch=?2 AND state='starting'",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64,
                command.now_millis,
                command.category.as_str()
            ],
        )?,
        "runtime epoch",
    )?;
    let next = entity_revision_from_i64(row.session_revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state=?2,residency='cold',revision=?3,
         projection_revision=?4,updated_at=?5 WHERE id=?1",
            params![
                command.session_id.as_str(),
                restored,
                next.get() as i64,
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "session",
    )?;
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "activationAborted",
        "entityId": command.session_id.as_str(),
        "state": restored,
        "runtimeEpoch": command.runtime_epoch.get(),
        "category": command.category.as_str()
    }))
    .map_err(|_| StoreError::Corrupt("activation abort serialization"))?;
    append_fact_payload(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        None,
        "activationAborted",
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
        state: if restored == "created" {
            SessionState::Created
        } else {
            SessionState::Ready
        },
        revision: next,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}
