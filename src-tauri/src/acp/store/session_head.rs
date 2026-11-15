use rusqlite::{Connection, OptionalExtension, Transaction, params};

use sage_acp_protocol::contract::{ProjectionRevision, SageAgentSessionId};

use super::{StoreError, sessions::increment_session_usage};

pub(super) struct HeadSession<'a> {
    pub(super) project: &'a str,
    pub(super) chat: &'a str,
    pub(super) ordinal: i64,
    pub(super) binding_id: &'a str,
    pub(super) binding_digest: &'a str,
    pub(super) harness_version: Option<&'a str>,
    pub(super) model_id: Option<&'a str>,
    /// Present only for a session bound to a native thread at install.
    pub(super) external_session_id: Option<&'a str>,
    /// The earlier session of the Chat whose native thread this one resumes.
    pub(super) resumes_session_id: Option<&'a str>,
    pub(super) capabilities_json: &'a str,
    /// `ready` for a session bound to a native thread, else `created`.
    pub(super) state: &'static str,
}

pub(super) fn install_head_session(
    transaction: &Transaction<'_>,
    session: HeadSession<'_>,
    projection: ProjectionRevision,
    now_millis: i64,
) -> Result<SageAgentSessionId, StoreError> {
    let session_id = SageAgentSessionId::new();
    transaction.execute(
        "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
           harness_version,model_id,external_session_id,capabilities_json,state,residency,
           legacy,revision,projection_revision,created_at,updated_at,terminal_at,
           resumes_session_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'cold',0,0,?12,?13,?13,NULL,?14)",
        params![
            session_id.as_str(),
            session.project,
            session.chat,
            session.ordinal,
            session.binding_id,
            session.binding_digest,
            session.harness_version,
            session.model_id,
            session.external_session_id,
            session.capabilities_json,
            session.state,
            projection.get() as i64,
            now_millis,
            session.resumes_session_id,
        ],
    )?;
    transaction.execute(
        "INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,
           item_bytes,fact_count,fact_bytes,payload_bytes,revision)
         VALUES(?1,?2,?3,0,0,0,0,0,0,0)",
        params![session.project, session.chat, session_id.as_str()],
    )?;
    increment_session_usage(transaction, session.project)?;
    transaction.execute(
        "DELETE FROM chat_session_heads WHERE project_id=?1 AND chat_id=?2",
        params![session.project, session.chat],
    )?;
    transaction.execute(
        "INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at)
         VALUES(?1,?2,?3,?4,?5)",
        params![
            session.project,
            session.chat,
            session_id.as_str(),
            projection.get() as i64,
            now_millis
        ],
    )?;
    Ok(session_id)
}

/// A Chat's last session once it has no head, with what a started session
/// must share with it to go on from it.
pub(crate) struct EndedHead {
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) binding_id: String,
    pub(crate) model_id: Option<String>,
}

/// The session a Chat without a head ran last: the next session started in
/// the Chat goes on from it. `None` while the Chat has a head, or before it
/// ran any session of its own.
pub(crate) fn ended_head(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<Option<EndedHead>, StoreError> {
    let row = connection
        .query_row(
            "SELECT s.id,s.binding_id,s.model_id FROM agent_sessions s
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.legacy=0
               AND NOT EXISTS(SELECT 1 FROM chat_session_heads h
                              WHERE h.project_id=?1 AND h.chat_id=?2)
             ORDER BY s.ordinal DESC LIMIT 1",
            params![project, chat],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    row.map(|(id, binding_id, model_id)| {
        Ok(EndedHead {
            session_id: SageAgentSessionId::parse(id)
                .map_err(|_| StoreError::Corrupt("ended session id"))?,
            binding_id,
            model_id,
        })
    })
    .transpose()
}

/// Whether `session` is the one its Chat's next send runs: the Chat's head,
/// or, while the Chat has none, its [`ended_head`].
pub(crate) fn runs_next_send(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_session_heads WHERE session_id=?1)
            OR (NOT EXISTS(SELECT 1 FROM chat_session_heads h
                           JOIN agent_sessions s
                             ON s.project_id=h.project_id AND s.chat_id=h.chat_id
                           WHERE s.id=?1)
                AND ?1=(SELECT l.id FROM agent_sessions s
                        JOIN agent_sessions l
                          ON l.project_id=s.project_id AND l.chat_id=s.chat_id
                        WHERE s.id=?1 AND l.legacy=0
                        ORDER BY l.ordinal DESC LIMIT 1))",
        [session.as_str()],
        |row| row.get(0),
    )
}

/// The session `session` goes on from while it has run no turn: the one its
/// Chat ran just before it, when that ran on the same binding. A Chat that
/// changed harness goes on from nothing.
pub(crate) fn preceding_session(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> Result<Option<SageAgentSessionId>, StoreError> {
    let row = connection
        .query_row(
            "SELECT p.id,p.binding_id=s.binding_id FROM agent_sessions s
             JOIN agent_sessions p ON p.project_id=s.project_id AND p.chat_id=s.chat_id
             WHERE s.id=?1 AND p.legacy=0 AND p.ordinal<s.ordinal
               AND NOT EXISTS(SELECT 1 FROM turns t WHERE t.session_id=s.id)
             ORDER BY p.ordinal DESC LIMIT 1",
            [session.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?;
    row.filter(|(_, same_binding)| *same_binding)
        .map(|(id, _)| {
            SageAgentSessionId::parse(id).map_err(|_| StoreError::Corrupt("preceding session id"))
        })
        .transpose()
}
