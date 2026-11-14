use rusqlite::{Transaction, params};

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
