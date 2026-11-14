use rusqlite::{Transaction, params};

use crate::acp_protocol::permissions::PermissionIntent;
use sage_core::limits::LIMITS;

use super::{
    StoreError,
    facts::{append_fact_payload, check_add},
    sessions::ensure_one,
};

pub(super) fn ensure_capacity(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    bytes: i64,
) -> Result<(), StoreError> {
    let session: (i64, i64, i64) = transaction.query_row(
        "SELECT item_count,item_bytes,payload_bytes FROM acp_session_usage WHERE session_id=?1",
        [intent.session_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let global: (i64, i64, i64) = transaction.query_row(
        "SELECT item_count,item_bytes,shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    check_add(
        session.0,
        1,
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
    check_add(global.0, 1, LIMITS.durable.items_global, "globalItems")?;
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
    )
}

pub(super) fn ensure_request_capacity(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    bytes: i64,
) -> Result<(), StoreError> {
    let per_session: (i64, i64) = transaction.query_row(
        "SELECT COUNT(*),COALESCE(SUM(payload_bytes),0) FROM (
           SELECT length(CAST(normalized_payload_json AS BLOB)) AS payload_bytes
           FROM server_requests WHERE session_id=?1 LIMIT 513
         )",
        [intent.session_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let global: (i64, i64) = transaction.query_row(
        "SELECT COUNT(*),COALESCE(SUM(payload_bytes),0) FROM (
           SELECT length(CAST(normalized_payload_json AS BLOB)) AS payload_bytes
           FROM server_requests LIMIT 4097
         )",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    check_add(
        per_session.0,
        1,
        LIMITS.durable.requests_per_session,
        "durableSessionRequests",
    )?;
    check_add(
        global.0,
        1,
        LIMITS.durable.requests_global,
        "durableGlobalRequests",
    )?;
    check_add(
        global.1,
        bytes,
        LIMITS.durable.request_bytes_global,
        "durableRequestBytes",
    )?;

    let live_session: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM server_requests
           WHERE session_id=?1 AND state NOT IN ('resolvedAllowed','resolvedDeclined','cancelled') LIMIT 9
         )",
        [intent.session_id.as_str()],
        |row| row.get(0),
    )?;
    let live_global: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM server_requests
           WHERE state NOT IN ('resolvedAllowed','resolvedDeclined','cancelled') LIMIT 17
         )",
        [],
        |row| row.get(0),
    )?;
    check_add(
        live_session,
        1,
        LIMITS.queues.server_requests_per_session,
        "liveSessionRequests",
    )?;
    check_add(
        live_global,
        1,
        LIMITS.queues.server_requests_global,
        "liveGlobalRequests",
    )
}

pub(super) fn ensure_absent(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
) -> Result<(), StoreError> {
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM items WHERE session_id=?1 AND runtime_epoch=?2
             AND kind='permissionRequest' AND external_item_id=?3 LIMIT 2
         )",
        params![
            intent.session_id.as_str(),
            intent.runtime_epoch.get() as i64,
            intent.external_request_id.as_str()
        ],
        |row| row.get(0),
    )?;
    if count == 0 {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "permissionItemIdentity",
        })
    }
}

pub(super) fn update_usage(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    bytes: i64,
) -> Result<(), StoreError> {
    ensure_one(
        transaction.execute(
            "UPDATE acp_session_usage SET item_count=item_count+1,item_bytes=item_bytes+?2,
             payload_bytes=payload_bytes+?2,revision=revision+1 WHERE session_id=?1",
            params![intent.session_id.as_str(), bytes],
        )?,
        "session usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_project_usage SET item_count=item_count+1,payload_bytes=payload_bytes+?2,
             revision=revision+1 WHERE project_id=?1",
            params![intent.project_id.as_str(), bytes],
        )?,
        "project usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_store_usage SET item_count=item_count+1,item_bytes=item_bytes+?1,
             shared_payload_bytes=shared_payload_bytes+?1,revision=revision+1 WHERE singleton=1",
            [bytes],
        )?,
        "global usage",
    )
}

pub(super) fn verify_pair(
    connection: &rusqlite::Connection,
    intent: &PermissionIntent,
    lifecycle: &str,
    tool_state: &str,
) -> Result<(), StoreError> {
    let mut statement = connection.prepare(
        "SELECT payload_json,payload_digest,lifecycle,tool_state FROM items
         WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
           AND runtime_epoch=?5 AND kind='permissionRequest' AND external_item_id=?6 LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            intent.project_id.as_str(),
            intent.chat_id.as_str(),
            intent.session_id.as_str(),
            intent.turn_id.as_str(),
            intent.runtime_epoch.get() as i64,
            intent.external_request_id.as_str()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    let mut matches = Vec::with_capacity(2);
    for row in rows {
        matches.push(row?);
    }
    let [row] = matches.as_slice() else {
        return Err(StoreError::Corrupt("permission request item pair"));
    };
    if row.0 != intent.normalized_payload_json
        || row.1 != intent.normalized_payload_digest
        || row.2 != lifecycle
        || row.3.as_deref() != Some(tool_state)
    {
        return Err(StoreError::Conflict {
            resource: "permissionItemIdentity",
        });
    }
    Ok(())
}

pub(super) fn append_fact(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    item_id: &crate::acp::contract::SageItemId,
    projection: crate::acp::contract::ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "permissionItem",
        "entityId": item_id.as_str(),
        "state": "waitingForApproval"
    }))
    .map_err(|_| StoreError::Corrupt("permission item fact"))?;
    append_fact_payload(
        transaction,
        intent.project_id.as_str(),
        intent.chat_id.as_str(),
        intent.session_id.as_str(),
        Some(intent.turn_id.as_str()),
        "permissionItem",
        &payload,
        projection,
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        contract::SageAgentSessionId,
        store::extended_tests::{permission, setup},
    };

    #[test]
    fn resolved_history_uses_durable_capacity_but_not_the_live_request_slots() {
        let mut connection = setup();
        for index in 0..8 {
            insert_request(&connection, index, "session-one", "resolvedDeclined");
        }
        for index in 8..15 {
            insert_request(&connection, index, "session-one", "received");
        }
        let intent = permission(&SageAgentSessionId::parse("session-one").unwrap());
        let bytes = intent.normalized_payload_json.len() as i64;
        let transaction = connection.transaction().unwrap();
        ensure_request_capacity(&transaction, &intent, bytes).unwrap();
        transaction.rollback().unwrap();

        insert_request(&connection, 15, "session-one", "received");
        let transaction = connection.transaction().unwrap();
        assert!(matches!(
            ensure_request_capacity(&transaction, &intent, bytes),
            Err(StoreError::Capacity {
                resource: "liveSessionRequests",
                limit: 8
            })
        ));
    }

    #[test]
    fn live_global_request_limit_is_independent_of_session_history() {
        let mut connection = setup();
        for index in 0..16 {
            insert_request(&connection, index, &format!("session-{index}"), "received");
        }
        let intent = permission(&SageAgentSessionId::parse("session-new").unwrap());
        let bytes = intent.normalized_payload_json.len() as i64;
        let transaction = connection.transaction().unwrap();
        assert!(matches!(
            ensure_request_capacity(&transaction, &intent, bytes),
            Err(StoreError::Capacity {
                resource: "liveGlobalRequests",
                limit: 16
            })
        ));
    }

    fn insert_request(
        connection: &rusqlite::Connection,
        index: usize,
        session_id: &str,
        state: &str,
    ) {
        connection
            .execute(
                "INSERT INTO server_requests(
                   id,session_id,state,normalized_payload_json
                 ) VALUES (?1,?2,?3,'{}')",
                params![format!("request-{index}"), session_id, state],
            )
            .unwrap();
    }
}
