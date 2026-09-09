use rusqlite::{Transaction, params};

use crate::acp::limits::LIMITS;

use crate::acp::contract::{ProjectionRevision, SageItemId};

use super::{
    ItemIngressCommand, StoreError,
    facts::{append_fact_payload, check_add},
    sessions::ensure_one,
    state::item_lifecycle_text,
};

pub(super) fn ensure_capacity(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
    count: i64,
    bytes: i64,
) -> Result<i64, StoreError> {
    let turn: (i64, i64, i64) = transaction.query_row(
        "SELECT COUNT(*),COALESCE(SUM(payload_bytes),0)
         + COALESCE((SELECT SUM(old_bytes+new_bytes) FROM acp_file_change_snapshots
            WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
              AND runtime_epoch=?5),0),COALESCE(MAX(ordinal),-1)+1
         FROM items WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
           AND runtime_epoch=?5",
        params![
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64
        ],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let session: (i64, i64, i64) = transaction.query_row(
        "SELECT item_count,item_bytes,payload_bytes FROM acp_session_usage WHERE session_id=?1",
        [command.session_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let global: (i64, i64, i64) = transaction.query_row(
        "SELECT item_count,item_bytes,shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    check_add(turn.0, count, LIMITS.wire.items_per_turn, "turnItems")?;
    check_add(
        turn.1,
        bytes,
        LIMITS.wire.item_bytes_per_turn,
        "turnItemBytes",
    )?;
    check_add(
        session.0,
        count,
        LIMITS.durable.items_per_session,
        "sessionItems",
    )?;
    check_add(
        session.1,
        bytes,
        LIMITS.durable.item_bytes_per_session,
        "sessionItemBytes",
    )?;
    check_add(
        session.2,
        bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "sessionPayload",
    )?;
    check_add(global.0, count, LIMITS.durable.items_global, "globalItems")?;
    check_add(
        global.1,
        bytes,
        LIMITS.durable.item_bytes_global,
        "globalItemBytes",
    )?;
    check_add(
        global.2,
        bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "globalPayload",
    )?;
    if turn.2 < 0 {
        return Err(StoreError::CounterExhausted);
    }
    Ok(turn.2)
}

pub(super) fn update(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
    count: i64,
    bytes: i64,
) -> Result<(), StoreError> {
    ensure_one(
        transaction.execute(
            "UPDATE acp_session_usage SET item_count=item_count+?2,item_bytes=item_bytes+?3,
             payload_bytes=payload_bytes+?3,revision=revision+1 WHERE session_id=?1",
            params![command.session_id.as_str(), count, bytes],
        )?,
        "session usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_project_usage SET item_count=item_count+?2,payload_bytes=payload_bytes+?3,
             revision=revision+1 WHERE project_id=?1",
            params![command.project_id.as_str(), count, bytes],
        )?,
        "project usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_store_usage SET item_count=item_count+?1,item_bytes=item_bytes+?2,
             shared_payload_bytes=shared_payload_bytes+?2,revision=revision+1 WHERE singleton=1",
            params![count, bytes],
        )?,
        "global usage",
    )
}

pub(super) fn append_fact(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
    item: &SageItemId,
    projection: ProjectionRevision,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "itemIngress",
        "entityId": item.as_str(),
        "state": item_lifecycle_text(command.lifecycle)
    }))
    .map_err(|_| StoreError::Corrupt("item fact"))?;
    append_fact_payload(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        Some(command.turn_id.as_str()),
        "itemIngress",
        &payload,
        projection,
        command.now_millis,
    )
}
