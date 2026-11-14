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

/// `pinned` names the runtime that listed earlier pages, which alone may
/// read a session it listed; without it, the most recently active one.
pub(crate) fn import_lister(
    connection: &Connection,
    project_id: &SageProjectId,
    pinned: Option<(&SageAgentSessionId, RuntimeEpoch)>,
) -> Result<Option<RuntimeOwner>, StoreError> {
    let (session, epoch) = pinned.unzip();
    let epoch = epoch.map(|epoch| epoch.get() as i64);
    let row: Option<(String, String, i64)> = connection
        .query_row(
            &format!(
                "SELECT s.chat_id,s.id,e.runtime_epoch FROM chat_session_heads h
                 JOIN agent_sessions s ON s.id=h.session_id AND s.project_id=h.project_id
                   AND s.chat_id=h.chat_id
                 JOIN session_runtime_epochs e ON e.session_id=s.id AND e.state='running'
                 WHERE h.project_id=?1 AND s.residency='hot' AND s.state IN ('ready','active')
                   AND {ADMITS_IMPORT} AND (?2 IS NULL OR (s.id=?2 AND e.runtime_epoch=?3))
                 ORDER BY s.updated_at DESC LIMIT 1"
            ),
            params![project_id.as_str(), session.map(|id| id.as_str()), epoch],
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

/// The caller has just created `chat` in the same
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

/// A Chat's imported session: its id, the runtime epoch its history is
/// shown under (its first runtime's, which is 1 until it runs) and, when
/// asked for, the history kept at import.
pub(crate) struct ImportedSession {
    pub(crate) session_id: String,
    pub(crate) runtime_epoch: i64,
    pub(crate) history_json: Option<String>,
}

pub(crate) fn imported_session(
    connection: &Connection,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    with_history: bool,
) -> Result<Option<ImportedSession>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT s.id,COALESCE((SELECT MIN(runtime_epoch) FROM session_runtime_epochs
               WHERE session_id=s.id),1),CASE WHEN ?3 THEN i.history_json END
             FROM acp_imported_histories i JOIN agent_sessions s ON s.id=i.session_id
             WHERE s.project_id=?1 AND s.chat_id=?2 ORDER BY s.ordinal LIMIT 1",
            params![project_id.as_str(), chat_id.as_str(), with_history],
            |row| {
                Ok(ImportedSession {
                    session_id: row.get(0)?,
                    runtime_epoch: row.get(1)?,
                    history_json: row.get(2)?,
                })
            },
        )
        .optional()?)
}

/// The native threads the Project has imported, newest first, at most the
/// 100 a listing asks the harness to leave out. A page is still filtered
/// by `held_threads`, since an import refuses any thread a session holds.
pub(crate) fn imported_threads(
    connection: &Connection,
    project_id: &SageProjectId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT s.external_session_id FROM acp_imported_histories i
         JOIN agent_sessions s ON s.id=i.session_id
         WHERE s.project_id=?1 AND s.external_session_id IS NOT NULL
         ORDER BY i.created_at DESC LIMIT 100",
    )?;
    let threads = statement
        .query_map([project_id.as_str()], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(threads)
}

pub(crate) const HELD_THREADS: &str = "SELECT external_session_id FROM agent_sessions
     WHERE project_id=?1 AND external_session_id IN (SELECT value FROM json_each(?2))";

pub(crate) fn held_threads(
    connection: &Connection,
    project_id: &SageProjectId,
    threads: &[&str],
) -> Result<std::collections::HashSet<String>, StoreError> {
    let threads =
        serde_json::to_string(threads).map_err(|_| StoreError::Corrupt("listed threads"))?;
    let mut statement = connection.prepare_cached(HELD_THREADS)?;
    let held = statement
        .query_map(params![project_id.as_str(), threads], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(held)
}
