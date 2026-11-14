use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use crate::acp::contract::{EntityRevision, ProjectionRevision};
use sage_core::limits::LIMITS;

use super::{FactPayload, StoreError, TurnMutationCommand};

pub(crate) fn append_fact(
    transaction: &Transaction<'_>,
    command: &TurnMutationCommand,
    kind: &'static str,
    state: &'static str,
    entity_id: &str,
    projection_revision: ProjectionRevision,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&FactPayload {
        kind: kind.into(),
        entity_id: entity_id.into(),
        state: state.into(),
    })
    .map_err(|_| StoreError::Corrupt("fact serialization"))?;
    append_fact_payload(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        Some(command.turn_id.as_str()),
        kind,
        &payload,
        projection_revision,
        command.now_millis,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_fact_payload(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    session_id: &str,
    turn_id: Option<&str>,
    kind: &str,
    payload: &str,
    projection_revision: ProjectionRevision,
    now_millis: i64,
) -> Result<(), StoreError> {
    let payload_bytes = i64::try_from(payload.len()).map_err(|_| StoreError::PayloadTooLarge)?;
    if payload_bytes > LIMITS.durable.fact_bytes_each as i64 {
        return Err(StoreError::PayloadTooLarge);
    }
    ensure_fact_capacity(transaction, session_id, payload_bytes)?;
    let sequence: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(sequence),0)+1 FROM session_facts WHERE session_id=?1",
        [session_id],
        |row| row.get(0),
    )?;
    if sequence <= 0 {
        return Err(StoreError::CounterExhausted);
    }
    transaction.execute(
        "INSERT INTO session_facts (project_id,chat_id,session_id,sequence,turn_id,kind,
         payload_json,payload_bytes,payload_digest,projection_revision,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            project_id,
            chat_id,
            session_id,
            sequence,
            turn_id,
            kind,
            payload,
            payload_bytes,
            hex_digest(payload.as_bytes()),
            projection_revision.get() as i64,
            now_millis
        ],
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_session_usage SET fact_count=fact_count+1,fact_bytes=fact_bytes+?2,
         payload_bytes=payload_bytes+?2,revision=revision+1 WHERE session_id=?1",
            params![session_id, payload_bytes],
        )?,
        "session usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_project_usage SET fact_count=fact_count+1,payload_bytes=payload_bytes+?2,
         revision=revision+1 WHERE project_id=?1",
            params![project_id, payload_bytes],
        )?,
        "project usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_store_usage SET fact_count=fact_count+1,fact_bytes=fact_bytes+?1,
         shared_payload_bytes=shared_payload_bytes+?1,revision=revision+1 WHERE singleton=1",
            [payload_bytes],
        )?,
        "global usage",
    )?;
    Ok(())
}

pub(crate) fn next_projection_revision(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
) -> Result<ProjectionRevision, StoreError> {
    let current: i64 = transaction
        .query_row(
            "SELECT projection_revision FROM chats WHERE project_id=?1 AND id=?2",
            params![project_id, chat_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "chat" })?;
    projection_from_i64(current)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)
}

pub(crate) fn publish_projection(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    session_id: &str,
    revision: ProjectionRevision,
    now_millis: i64,
) -> Result<(), StoreError> {
    ensure_one(transaction.execute(
        "UPDATE chats SET projection_revision=?3,updated_at=?4 WHERE project_id=?1 AND id=?2 AND projection_revision<?3",
        params![project_id,chat_id,revision.get() as i64,now_millis],
    )?,"chat projection")?;
    crate::turn_workspace::settle_terminal(transaction, session_id)?;
    ensure_one(transaction.execute(
        "UPDATE agent_sessions SET projection_revision=?2 WHERE id=?1 AND projection_revision<?2",
        params![session_id,revision.get() as i64],
    )?,"session projection")
}

/// Publishes a projection change that leaves a racing send current. Every
/// projection change advances `chats.intent_revision` (a schema trigger), so
/// a send-neutral writer restores the prior value in the same transaction.
pub(crate) fn publish_send_neutral_projection(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    session_id: &str,
    revision: ProjectionRevision,
    now_millis: i64,
) -> Result<(), StoreError> {
    let intent: i64 = transaction.query_row(
        "SELECT intent_revision FROM chats WHERE project_id=?1 AND id=?2",
        params![project_id, chat_id],
        |row| row.get(0),
    )?;
    publish_projection(
        transaction,
        project_id,
        chat_id,
        session_id,
        revision,
        now_millis,
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE chats SET intent_revision=?3 WHERE project_id=?1 AND id=?2",
            params![project_id, chat_id, intent],
        )?,
        "chat intent",
    )
}

pub(crate) fn ensure_fact_capacity(
    transaction: &Transaction<'_>,
    session_id: &str,
    bytes: i64,
) -> Result<(), StoreError> {
    let session: (i64, i64, i64) = transaction
        .query_row(
            "SELECT fact_count,fact_bytes,payload_bytes FROM acp_session_usage WHERE session_id=?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Corrupt("missing session usage"))?;
    let global:(i64,i64,i64)=transaction.query_row(
        "SELECT fact_count,fact_bytes,shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
        [],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).optional()?.ok_or(StoreError::Corrupt("missing global usage"))?;
    check_add(
        session.0,
        1,
        LIMITS.durable.facts_per_session,
        "sessionFacts",
    )?;
    check_add(
        session.1,
        bytes,
        LIMITS.durable.fact_bytes_global,
        "sessionFactBytes",
    )?;
    check_add(
        session.2,
        bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "sessionPayload",
    )?;
    check_add(global.0, 1, LIMITS.durable.facts_global, "globalFacts")?;
    check_add(
        global.1,
        bytes,
        LIMITS.durable.fact_bytes_global,
        "globalFactBytes",
    )?;
    check_add(
        global.2,
        bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "globalPayload",
    )
}

pub(crate) fn check_add(
    current: i64,
    added: i64,
    limit: u64,
    resource: &'static str,
) -> Result<(), StoreError> {
    if current < 0
        || added < 0
        || current
            .checked_add(added)
            .is_none_or(|next| next > limit as i64)
    {
        Err(StoreError::Capacity { resource, limit })
    } else {
        Ok(())
    }
}

pub(crate) fn entity_revision_from_i64(value: i64) -> Result<EntityRevision, StoreError> {
    EntityRevision::new(u64::try_from(value).map_err(|_| StoreError::Corrupt("entity revision"))?)
        .map_err(|_| StoreError::Corrupt("entity revision"))
}

pub(crate) fn projection_from_i64(value: i64) -> Result<ProjectionRevision, StoreError> {
    ProjectionRevision::new(
        u64::try_from(value).map_err(|_| StoreError::Corrupt("projection revision"))?,
    )
    .map_err(|_| StoreError::Corrupt("projection revision"))
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}
