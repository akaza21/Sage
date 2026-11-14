//! Restarts a Chat whose only native history was undone. A native thread
//! cannot be cut before its first Turn, and a session's thread is fixed
//! once it is bound, so the parked head is closed and a fresh head on the
//! same binding and model takes its place: its first activation starts a
//! new native session.

use rusqlite::{OptionalExtension, params};

use crate::acp::{
    contract::{EntityRevision, ProjectionRevision, SageAgentSessionId, SageChatId, SageProjectId},
    lifecycle::SessionState,
};

use super::{
    MutationDisposition, StoreError,
    facts::{append_fact_payload, entity_revision_from_i64, next_projection_revision},
    session_head::{HeadSession, install_head_session},
    session_resume::{ParkedSession, close_parked, unstarted_continuation},
    sessions::{
        PENDING_CAPABILITIES_JSON, SessionMutation, ensure_session_capacity, update_chat,
        validate_chat_revision,
    },
    turn_undo::{UndoneThread, pending_thread_rewind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestartSessionCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub session_id: SageAgentSessionId,
    pub now_millis: i64,
}

struct ParkedHead {
    binding_id: String,
    binding_digest: String,
    harness_version: Option<String>,
    model_id: Option<String>,
    revision: i64,
}

/// Replaces the parked head with a fresh `created` head on the same
/// binding, linked to nothing. Repeating it while that head is still
/// `created` returns it as a duplicate.
pub(crate) fn restart_undone_session(
    connection: &mut rusqlite::Connection,
    command: RestartSessionCommand,
) -> Result<SessionMutation, StoreError> {
    let project = command.project_id.as_str();
    let chat = command.chat_id.as_str();
    let earlier = command.session_id.as_str();
    let transaction = connection.transaction()?;
    if let Some(restarted) = unstarted_continuation(&transaction, project, chat, earlier)? {
        return Ok(restarted);
    }
    validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    let conflict = StoreError::Conflict {
        resource: "restartedSession",
    };
    if pending_thread_rewind(&transaction, earlier)?
        .is_none_or(|rewind| rewind.thread != UndoneThread::Restart)
    {
        return Err(conflict);
    }
    let parked = transaction
        .query_row(
            "SELECT s.binding_id,s.binding_digest,s.harness_version,s.model_id,s.revision
             FROM agent_sessions s JOIN chat_session_heads h ON h.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3
               AND s.state='ready' AND s.residency='cold'
               AND NOT EXISTS(SELECT 1 FROM session_runtime_epochs e WHERE e.session_id=s.id
                 AND e.state IN ('starting','running','terminating'))",
            params![project, chat, earlier],
            |row| {
                Ok(ParkedHead {
                    binding_id: row.get(0)?,
                    binding_digest: row.get(1)?,
                    harness_version: row.get(2)?,
                    model_id: row.get(3)?,
                    revision: row.get(4)?,
                })
            },
        )
        .optional()?
        .ok_or(conflict)?;
    ensure_session_capacity(&transaction, &command.project_id, &command.chat_id)?;
    let ordinal: i64 = transaction.query_row(
        "SELECT MAX(ordinal)+1 FROM agent_sessions WHERE chat_id=?1",
        [chat],
        |row| row.get(0),
    )?;
    let projection = next_projection_revision(&transaction, project, chat)?;
    close_parked(
        &transaction,
        ParkedSession {
            project_id: &command.project_id,
            chat_id: &command.chat_id,
            session_id: &command.session_id,
            revision: entity_revision_from_i64(parked.revision)?,
        },
        projection,
        command.now_millis,
    )?;
    let session_id = install_head_session(
        &transaction,
        HeadSession {
            project,
            chat,
            ordinal,
            binding_id: &parked.binding_id,
            binding_digest: &parked.binding_digest,
            harness_version: parked.harness_version.as_deref(),
            model_id: parked.model_id.as_deref(),
            external_session_id: None,
            resumes_session_id: None,
            capabilities_json: PENDING_CAPABILITIES_JSON,
            state: "created",
        },
        projection,
        command.now_millis,
    )?;
    crate::acp::session_config::choices::copy(&transaction, &command.session_id, &session_id)?;
    let payload = serde_json::json!({
        "kind": "sessionRestarted",
        "entityId": session_id.as_str(),
        "state": "committed",
        "replacesSessionId": earlier,
    })
    .to_string();
    append_fact_payload(
        &transaction,
        project,
        chat,
        session_id.as_str(),
        None,
        "sessionRestarted",
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
        session_id,
        runtime_epoch: None,
        state: SessionState::Created,
        revision: EntityRevision::ZERO,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}
