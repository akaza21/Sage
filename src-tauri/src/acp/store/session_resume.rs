//! Continues a Chat whose head session was bound by an earlier harness
//! release. A Chat holds one live session, so the parked earlier session is
//! closed; its identity and everything recorded under it stay as they are.
//! A new head session on the current binding links to it, and its first
//! activation resumes the linked native thread.

use rusqlite::{OptionalExtension, params};

use crate::acp::{
    contract::{ProjectionRevision, SafeText, SageAgentSessionId, SageChatId, SageProjectId},
    lifecycle::SessionState,
};

use super::{
    MutationDisposition, StoreError,
    facts::{
        append_fact_payload, entity_revision_from_i64, next_projection_revision,
        projection_from_i64,
    },
    session_head::{HeadSession, install_head_session},
    sessions::{
        PENDING_CAPABILITIES_JSON, SessionMutation, ensure_one, ensure_session_capacity,
        update_chat, valid_digest, validate_chat_revision,
    },
    terminal::append_entity_fact,
};
use crate::acp::contract::EntityRevision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeEarlierSessionCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    /// The Chat's current head, bound by an earlier harness release.
    pub earlier_session_id: SageAgentSessionId,
    pub binding_digest: String,
    pub harness_version: SafeText<128>,
    pub now_millis: i64,
}

/// Installs a `created` head session on the current binding that resumes
/// the earlier head's native thread. Retrying returns the installed session.
pub(crate) fn link_resumed_session(
    connection: &mut rusqlite::Connection,
    command: ResumeEarlierSessionCommand,
) -> Result<SessionMutation, StoreError> {
    if !valid_digest(&command.binding_digest) {
        return Err(StoreError::Corrupt("binding digest"));
    }
    let project = command.project_id.as_str();
    let chat = command.chat_id.as_str();
    let earlier = command.earlier_session_id.as_str();
    let transaction = connection.transaction()?;
    if let Some(linked) = installed_link(&transaction, project, chat, earlier)? {
        return Ok(linked);
    }
    validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    let (binding_id, model_id, revision): (String, Option<String>, i64) = transaction
        .query_row(
            "SELECT s.binding_id,s.model_id,s.revision FROM agent_sessions s
             JOIN chat_session_heads h ON h.session_id=s.id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3
               AND s.state='ready' AND s.residency='cold'
               AND s.external_session_id IS NOT NULL",
            params![project, chat, earlier],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "resumedSession",
        })?;
    if transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_runtime_epochs WHERE session_id=?1
           AND state IN ('starting','running','terminating'))",
        [earlier],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(StoreError::Conflict {
            resource: "resumedSession",
        });
    }
    ensure_session_capacity(&transaction, &command.project_id, &command.chat_id)?;
    let ordinal: i64 = transaction.query_row(
        "SELECT MAX(ordinal)+1 FROM agent_sessions WHERE chat_id=?1",
        [chat],
        |row| row.get(0),
    )?;
    let projection = next_projection_revision(&transaction, project, chat)?;
    close_parked(
        &transaction,
        &command,
        entity_revision_from_i64(revision)?,
        projection,
    )?;
    let session_id = install_head_session(
        &transaction,
        HeadSession {
            project,
            chat,
            ordinal,
            binding_id: &binding_id,
            binding_digest: &command.binding_digest,
            harness_version: Some(command.harness_version.as_str()),
            model_id: model_id.as_deref(),
            external_session_id: None,
            resumes_session_id: Some(earlier),
            capabilities_json: PENDING_CAPABILITIES_JSON,
            state: "created",
        },
        projection,
        command.now_millis,
    )?;
    let payload = serde_json::json!({
        "kind": "sessionResumeLinked",
        "entityId": session_id.as_str(),
        "state": "committed",
        "resumesSessionId": earlier,
    })
    .to_string();
    append_fact_payload(
        &transaction,
        project,
        chat,
        session_id.as_str(),
        None,
        "sessionResumeLinked",
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

/// Closes the parked earlier session: its head is removed first, then it
/// moves through `closing` to `closed`. Nothing else about it changes.
fn close_parked(
    transaction: &rusqlite::Transaction<'_>,
    command: &ResumeEarlierSessionCommand,
    revision: EntityRevision,
    projection: ProjectionRevision,
) -> Result<(), StoreError> {
    let earlier = command.earlier_session_id.as_str();
    let next = revision.next().map_err(|_| StoreError::CounterExhausted)?;
    ensure_one(
        transaction.execute(
            "DELETE FROM chat_session_heads WHERE session_id=?1",
            [earlier],
        )?,
        "chat head",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='closing' WHERE id=?1 AND state='ready'",
            [earlier],
        )?,
        "session",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='closed',revision=?2,projection_revision=?3,
             updated_at=?4,terminal_at=?4 WHERE id=?1 AND state='closing'",
            params![
                earlier,
                next.get() as i64,
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "session",
    )?;
    append_entity_fact(
        transaction,
        &command.project_id,
        &command.chat_id,
        &command.earlier_session_id,
        None,
        "sessionTerminal",
        earlier,
        "closed",
        projection,
        command.now_millis,
    )
}

/// The Chat's head when it already resumes `earlier` and has not started.
fn installed_link(
    transaction: &rusqlite::Transaction<'_>,
    project: &str,
    chat: &str,
    earlier: &str,
) -> Result<Option<SessionMutation>, StoreError> {
    let row: Option<(String, i64, i64)> = transaction
        .query_row(
            "SELECT s.id,s.revision,c.projection_revision FROM agent_sessions s
             JOIN chat_session_heads h ON h.session_id=s.id
             JOIN chats c ON c.project_id=s.project_id AND c.id=s.chat_id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.resumes_session_id=?3
               AND s.state='created'",
            params![project, chat, earlier],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(id, revision, projection)| {
        Ok(SessionMutation {
            session_id: SageAgentSessionId::parse(id)
                .map_err(|_| StoreError::Corrupt("session id"))?,
            runtime_epoch: None,
            state: SessionState::Created,
            revision: entity_revision_from_i64(revision)?,
            projection_revision: projection_from_i64(projection)?,
            disposition: MutationDisposition::Duplicate,
        })
    })
    .transpose()
}

#[cfg(test)]
mod tests;
