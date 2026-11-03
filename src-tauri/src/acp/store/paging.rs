use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    lifecycle::SessionState,
};

use super::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryCandidate {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub session_state: SessionState,
    pub runtime_epoch: RuntimeEpoch,
}

pub(crate) fn load_recovery_candidates(
    connection: &rusqlite::Connection,
) -> Result<Vec<RecoveryCandidate>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT s.project_id, s.chat_id, s.id, s.state, e.runtime_epoch
         FROM session_runtime_epochs e
         JOIN agent_sessions s ON s.id=e.session_id
         WHERE e.state IN ('starting','running','terminating')
         ORDER BY e.started_at, s.id
         LIMIT ?1",
    )?;
    let live_tree_limit = crate::acp::limits::LIMITS.process.operational_live_trees as usize;
    let rows = statement.query_map([live_tree_limit as i64 + 1], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut candidates = Vec::with_capacity(live_tree_limit);
    for row in rows {
        if candidates.len() == live_tree_limit {
            return Err(StoreError::Corrupt(
                "live runtime trees exceed the bounded limit",
            ));
        }
        let row = row?;
        let runtime_epoch = u64::try_from(row.4)
            .ok()
            .and_then(|value| RuntimeEpoch::new(value).ok())
            .ok_or(StoreError::Corrupt("runtime epoch"))?;
        candidates.push(RecoveryCandidate {
            project_id: SageProjectId::parse(row.0)
                .map_err(|_| StoreError::Corrupt("project id"))?,
            chat_id: SageChatId::parse(row.1).map_err(|_| StoreError::Corrupt("chat id"))?,
            session_id: SageAgentSessionId::parse(row.2)
                .map_err(|_| StoreError::Corrupt("session id"))?,
            session_state: parse_session_state(&row.3)?,
            runtime_epoch,
        });
    }
    Ok(candidates)
}

fn parse_session_state(value: &str) -> Result<SessionState, StoreError> {
    match value {
        "created" => Ok(SessionState::Created),
        "starting" => Ok(SessionState::Starting),
        "ready" => Ok(SessionState::Ready),
        "active" => Ok(SessionState::Active),
        "recovering" => Ok(SessionState::Recovering),
        "closing" => Ok(SessionState::Closing),
        "closed" => Ok(SessionState::Closed),
        "failed" => Ok(SessionState::Failed),
        "incompatible" => Ok(SessionState::Incompatible),
        _ => Err(StoreError::Corrupt("session state")),
    }
}
