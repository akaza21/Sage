//! Durable import of a native session started outside Sage. Like a native
//! fork, the import installs a resumable head session bound to the native
//! thread in the transaction that creates its Chat, so the first send
//! resumes it; the history read at import is kept beside that session.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    intent::ExternalId,
    scheduler::RuntimeOwner,
};

use super::{
    StoreError,
    facts::{append_fact_payload, next_projection_revision},
    session_head::{HeadSession, install_head_session},
    sessions::{ensure_session_capacity, update_chat},
};

const ADMITS_IMPORT: &str = "json_extract(s.capabilities_json,'$.nativeSessionImport')=1";

/// The live runtime that lists and reads sessions to import for a Project:
/// the most recently active hot head session whose own capabilities admit
/// `nativeSessionImport`.
pub(crate) fn import_lister(
    connection: &Connection,
    project_id: &SageProjectId,
) -> Result<Option<RuntimeOwner>, StoreError> {
    let row: Option<(String, String, i64)> = connection
        .query_row(
            &format!(
                "SELECT s.chat_id,s.id,e.runtime_epoch FROM chat_session_heads h
                 JOIN agent_sessions s ON s.id=h.session_id AND s.project_id=h.project_id
                   AND s.chat_id=h.chat_id
                 JOIN session_runtime_epochs e ON e.session_id=s.id AND e.state='running'
                 WHERE h.project_id=?1 AND s.residency='hot' AND s.state IN ('ready','active')
                   AND {ADMITS_IMPORT}
                 ORDER BY s.updated_at DESC LIMIT 1"
            ),
            [project_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(chat, session, epoch)| {
        Ok(RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: SageChatId::parse(chat).map_err(|_| StoreError::Corrupt("lister chat id"))?,
            session_id: SageAgentSessionId::parse(session)
                .map_err(|_| StoreError::Corrupt("lister session id"))?,
            runtime_epoch: u64::try_from(epoch)
                .ok()
                .and_then(|value| RuntimeEpoch::new(value).ok())
                .ok_or(StoreError::Corrupt("lister runtime epoch"))?,
        })
    })
    .transpose()
}

/// Installs the imported session as the head of `chat` (cold, `ready`,
/// bound to `thread` with the lister's binding and capabilities) and keeps
/// its `history_json`. The caller has just created `chat` in the same
/// transaction. A thread a Project session already holds is refused.
pub(crate) fn install_import(
    transaction: &Transaction<'_>,
    lister: &RuntimeOwner,
    chat: &SageChatId,
    thread: &ExternalId,
    history_json: &str,
    now_millis: i64,
) -> Result<SageAgentSessionId, StoreError> {
    let project = lister.project_id.as_str();
    let held: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE project_id=?1
           AND external_session_id=?2)",
        params![project, thread.as_str()],
        |row| row.get(0),
    )?;
    if held {
        return Err(StoreError::Conflict {
            resource: "importedSession",
        });
    }
    let (binding_id, binding_digest, harness_version, capabilities): (
        String,
        String,
        Option<String>,
        String,
    ) = transaction
        .query_row(
            &format!(
                "SELECT binding_id,binding_digest,harness_version,capabilities_json
                 FROM agent_sessions s WHERE id=?1 AND project_id=?2 AND {ADMITS_IMPORT}"
            ),
            params![lister.session_id.as_str(), project],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "importLister",
        })?;
    ensure_session_capacity(transaction, &lister.project_id, chat)?;
    let projection = next_projection_revision(transaction, project, chat.as_str())?;
    let session_id = install_head_session(
        transaction,
        HeadSession {
            project,
            chat: chat.as_str(),
            ordinal: 0,
            binding_id: &binding_id,
            binding_digest: &binding_digest,
            harness_version: harness_version.as_deref(),
            // The resumed thread keeps the model it was last run with.
            model_id: None,
            external_session_id: Some(thread.as_str()),
            resumes_session_id: None,
            capabilities_json: &capabilities,
            state: "ready",
        },
        projection,
        now_millis,
    )?;
    transaction.execute(
        "INSERT INTO acp_imported_histories(session_id,history_json,created_at) VALUES(?1,?2,?3)",
        params![session_id.as_str(), history_json, now_millis],
    )?;
    let payload = serde_json::json!({
        "kind": "sessionImported",
        "entityId": session_id.as_str(),
        "state": "committed",
    })
    .to_string();
    append_fact_payload(
        transaction,
        project,
        chat.as_str(),
        session_id.as_str(),
        None,
        "sessionImported",
        &payload,
        projection,
        now_millis,
    )?;
    update_chat(
        transaction,
        &lister.project_id,
        chat,
        projection,
        now_millis,
    )?;
    Ok(session_id)
}

/// The history kept for the imported session of a Chat, if it has one.
pub(crate) fn imported_history(
    connection: &Connection,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> Result<Option<String>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT i.history_json FROM acp_imported_histories i
             JOIN agent_sessions s ON s.id=i.session_id
             WHERE s.project_id=?1 AND s.chat_id=?2 ORDER BY s.ordinal LIMIT 1",
            params![project_id.as_str(), chat_id.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn has_imported_history(
    connection: &Connection,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> Result<bool, StoreError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM acp_imported_histories i
           JOIN agent_sessions s ON s.id=i.session_id
           WHERE s.project_id=?1 AND s.chat_id=?2)",
        params![project_id.as_str(), chat_id.as_str()],
        |row| row.get(0),
    )?)
}
