use super::{
    StartSessionCommand, StoreError,
    facts::next_projection_revision,
    sessions::{PENDING_CAPABILITIES_JSON, increment_session_usage},
};
use crate::acp::contract::{ProjectionRevision, SageAgentSessionId};
use rusqlite::{Transaction, params};

pub(super) fn insert_session_in(
    transaction: &Transaction<'_>,
    command: &StartSessionCommand,
) -> Result<(SageAgentSessionId, ProjectionRevision), StoreError> {
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal),-1)+1 FROM agent_sessions WHERE chat_id=?1",
        [command.chat_id.as_str()],
        |row| row.get(0),
    )?;
    if ordinal < 0 {
        return Err(StoreError::CounterExhausted);
    }
    let session_id = SageAgentSessionId::new();
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    transaction.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,harness_version,model_id,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at,terminal_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,'created','cold',0,0,?10,?11,?11,NULL)",params![session_id.as_str(),command.project_id.as_str(),command.chat_id.as_str(),ordinal,command.binding_id.as_str(),command.binding_digest,command.harness_version.as_ref().map(|v|v.as_str()),command.model_id.as_ref().map(|v|v.as_str()),PENDING_CAPABILITIES_JSON,projection.get() as i64,command.now_millis])?;
    transaction.execute("INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision) VALUES(?1,?2,?3,0,0,0,0,0,0,0)",params![command.project_id.as_str(),command.chat_id.as_str(),session_id.as_str()])?;
    increment_session_usage(transaction, command.project_id.as_str())?;
    transaction.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES(?1,?2,?3,?4,?5)",params![command.project_id.as_str(),command.chat_id.as_str(),session_id.as_str(),projection.get() as i64,command.now_millis])?;
    Ok((session_id, projection))
}
