use rusqlite::{Transaction, params};

use crate::acp::{
    intent::NormalizedPayload, limits::LIMITS, runtime::permissions::PermissionIntent,
};

use super::{StoreError, facts::hex_digest};

pub(super) struct RequestRow {
    id: String,
    project_id: String,
    chat_id: String,
    session_id: String,
    turn_id: String,
    runtime_epoch: i64,
    binding_digest: String,
    external_id: String,
    kind: String,
    payload_digest: String,
    payload_json: String,
    pub(super) state: String,
    pub(super) revision: i64,
}

pub(super) fn find(
    connection: &rusqlite::Connection,
    intent: &PermissionIntent,
) -> Result<Option<RequestRow>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
                external_request_id,kind,payload_digest,normalized_payload_json,state,revision
         FROM server_requests WHERE id=?1 OR (
           binding_digest=?2 AND session_id=?3 AND runtime_epoch=?4
           AND external_request_id=?5 AND kind=?6
         ) ORDER BY id LIMIT 2",
    )?;
    let rows = statement.query_map(
        params![
            intent.request_id.as_str(),
            intent.binding_digest,
            intent.session_id.as_str(),
            intent.runtime_epoch.get() as i64,
            intent.external_request_id.as_str(),
            intent.kind.as_str()
        ],
        |row| {
            Ok(RequestRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                chat_id: row.get(2)?,
                session_id: row.get(3)?,
                turn_id: row.get(4)?,
                runtime_epoch: row.get(5)?,
                binding_digest: row.get(6)?,
                external_id: row.get(7)?,
                kind: row.get(8)?,
                payload_digest: row.get(9)?,
                payload_json: row.get(10)?,
                state: row.get(11)?,
                revision: row.get(12)?,
            })
        },
    )?;
    let mut matches = Vec::with_capacity(2);
    for row in rows {
        matches.push(row?);
    }
    if matches.len() > 1 {
        return Err(StoreError::Conflict {
            resource: "requestIdentity",
        });
    }
    Ok(matches.pop())
}

pub(super) fn verify(row: &RequestRow, intent: &PermissionIntent) -> Result<(), StoreError> {
    if row.id != intent.request_id.as_str()
        || row.project_id != intent.project_id.as_str()
        || row.chat_id != intent.chat_id.as_str()
        || row.session_id != intent.session_id.as_str()
        || row.turn_id != intent.turn_id.as_str()
        || row.runtime_epoch != intent.runtime_epoch.get() as i64
        || row.binding_digest != intent.binding_digest
        || row.external_id != intent.external_request_id.as_str()
        || row.kind != intent.kind.as_str()
        || row.payload_digest != intent.payload_digest
        || row.payload_json != intent.normalized_payload_json
    {
        return Err(StoreError::Conflict {
            resource: "requestIdentity",
        });
    }
    Ok(())
}

pub(super) fn verify_owner(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
) -> Result<(), StoreError> {
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns t
         JOIN agent_sessions s ON s.id=t.session_id
         JOIN session_runtime_epochs e ON e.session_id=t.session_id
           AND e.runtime_epoch=t.runtime_epoch
         WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4
           AND t.runtime_epoch=?5 AND s.binding_digest=?6 AND s.state IN ('ready','active')
           AND e.state='running' AND t.state NOT IN (
             'completed','interrupted','cancelled','failed','deliveryUnknown'
           ))",
        params![
            intent.project_id.as_str(),
            intent.chat_id.as_str(),
            intent.session_id.as_str(),
            intent.turn_id.as_str(),
            intent.runtime_epoch.get() as i64,
            intent.binding_digest
        ],
        |row| row.get(0),
    )?;
    if valid {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "requestOwner",
        })
    }
}

pub(super) fn validate_payload(intent: &PermissionIntent) -> Result<(), StoreError> {
    if !valid_digest(&intent.binding_digest)
        || !valid_digest(&intent.payload_digest)
        || !valid_digest(&intent.normalized_payload_digest)
        || intent.normalized_payload_json.len() > LIMITS.wire.permission_payload_bytes as usize
        || hex_digest(intent.normalized_payload_json.as_bytes()) != intent.normalized_payload_digest
    {
        return Err(StoreError::Corrupt("request payload"));
    }
    let payload = serde_json::from_str::<NormalizedPayload>(&intent.normalized_payload_json)
        .map_err(|_| StoreError::Corrupt("request payload"))?;
    if !matches!(payload, NormalizedPayload::PermissionRequest { .. })
        || !matches!(intent.kind.as_str(), "permission" | "browserPermission")
    {
        return Err(StoreError::Corrupt("request payload"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
