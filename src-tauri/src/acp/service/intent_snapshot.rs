use rusqlite::{OptionalExtension, params};

use crate::{
    acp::{
        contract::{
            EntityRevision, ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId,
            SageProjectId, SageTurnId,
        },
        intent::IntentSnapshot,
        store::{StoreError, parse_turn_state},
    },
    persistence::Database,
};

type SnapshotRow = (
    i64,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

pub(super) fn load_intent_snapshot(
    database: &Database,
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
) -> Result<IntentSnapshot, StoreError> {
    let row: SnapshotRow = database
        .connection()
        .query_row(
            "SELECT c.projection_revision,h.session_id,e.runtime_epoch,t.id,t.state,t.revision
             FROM chats c
             LEFT JOIN chat_session_heads h
               ON h.project_id=c.project_id AND h.chat_id=c.id
             LEFT JOIN session_runtime_epochs e
               ON e.project_id=c.project_id AND e.chat_id=c.id AND e.session_id=h.session_id
              AND e.state IN ('starting','running','terminating')
             LEFT JOIN turns t
               ON t.project_id=c.project_id AND t.chat_id=c.id AND t.session_id=h.session_id
              AND t.state IN ('queued','sending','running','waitingModel','waitingTool',
                              'waitingApproval','waitingUser','cancelling')
             WHERE c.project_id=?1 AND c.id=?2",
            params![project_id.as_str(), chat_id.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "chat" })?;
    Ok(IntentSnapshot {
        project_id,
        project_generation,
        chat_id,
        projection_revision: crate::acp::contract::ProjectionRevision::new(checked(
            row.0,
            "projection revision",
        )?)
        .map_err(|_| StoreError::Corrupt("projection revision"))?,
        session_id: row
            .1
            .map(SageAgentSessionId::parse)
            .transpose()
            .map_err(|_| StoreError::Corrupt("session id"))?,
        runtime_epoch: row
            .2
            .map(|value| {
                RuntimeEpoch::new(checked(value, "runtime epoch")?)
                    .map_err(|_| StoreError::Corrupt("runtime epoch"))
            })
            .transpose()?,
        turn_id: row
            .3
            .map(SageTurnId::parse)
            .transpose()
            .map_err(|_| StoreError::Corrupt("turn id"))?,
        turn_state: row.4.as_deref().map(parse_turn_state).transpose()?,
        turn_revision: row
            .5
            .map(|value| {
                EntityRevision::new(checked(value, "turn revision")?)
                    .map_err(|_| StoreError::Corrupt("turn revision"))
            })
            .transpose()?,
    })
}

fn checked(value: i64, resource: &'static str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(resource))
}
