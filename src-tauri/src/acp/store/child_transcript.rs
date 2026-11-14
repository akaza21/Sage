use rusqlite::{Connection, TransactionBehavior, params};

use crate::acp::scheduler::RuntimeOwner;

use crate::acp_protocol::codex_protocol::{ChildTranscriptSnapshot, MAX_SNAPSHOT_BYTES};

mod live;
pub(crate) use live::persist_live_item_in;
pub(crate) use live::persist_turn_status_in;
use live::{merge_snapshots, validate_bounds};

const MAX_SPAWN_SCAN: i64 = 256;

/// The model a child's latest spawn requested, exactly as the wire named it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpawnModel {
    pub(crate) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<String>,
}

/// Scans at most the owner's latest 256 spawn rows; the newest spawn of this
/// child wins, matching the one-entry-per-spawn-invocation counting rule.
pub(crate) fn load_spawn_model(
    connection: &Connection,
    owner: &RuntimeOwner,
    child_thread_id: &str,
) -> Result<Option<SpawnModel>, ChildTranscriptStoreError> {
    use crate::acp::intent::NormalizedPayload;
    let mut statement = connection.prepare(
        "SELECT payload_json,payload_digest FROM items
         WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4
           AND kind='collaboration' AND instr(payload_json,'spawnAgent')>0
         ORDER BY rowid DESC LIMIT ?5",
    )?;
    let mut rows = statement.query(params![
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
        owner.runtime_epoch.get() as i64,
        MAX_SPAWN_SCAN,
    ])?;
    while let Some(row) = rows.next()? {
        let (encoded, digest): (String, String) = (row.get(0)?, row.get(1)?);
        if super::facts::hex_digest(encoded.as_bytes()) != digest {
            return Err(ChildTranscriptStoreError::Corrupt);
        }
        let Ok(NormalizedPayload::Collaboration {
            operation: Some(operation),
            agent_keys,
            model,
            reasoning_effort,
            ..
        }) = serde_json::from_str(&encoded)
        else {
            continue;
        };
        if operation.as_str() == "spawnAgent"
            && agent_keys
                .as_slice()
                .iter()
                .any(|key| key.as_str() == child_thread_id)
        {
            return Ok(model.map(|model| SpawnModel {
                model: model.as_str().to_owned(),
                reasoning_effort: reasoning_effort.map(|value| value.as_str().to_owned()),
            }));
        }
    }
    Ok(None)
}

#[derive(Clone)]
pub(crate) struct ChildTranscriptScope {
    pub(crate) owner: RuntimeOwner,
    pub(crate) item_id: String,
    pub(crate) child_thread_id: String,
    pub(crate) parent_thread_id: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ChildTranscriptStoreError {
    #[error("child transcript binding is stale or foreign")]
    Stale,
    #[error("child transcript storage failed")]
    Storage(#[from] rusqlite::Error),
    #[error("child transcript snapshot is corrupt")]
    Corrupt,
    #[error("child transcript capacity reached")]
    Capacity,
}

pub(crate) fn verify_current_binding(
    connection: &Connection,
    scope: &ChildTranscriptScope,
) -> Result<(), ChildTranscriptStoreError> {
    let epoch = i64::try_from(scope.owner.runtime_epoch.get())
        .map_err(|_| ChildTranscriptStoreError::Stale)?;
    let found: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM acp_child_thread_bindings b
           JOIN acp_child_thread_owners o ON o.native_child_thread_id=b.native_child_thread_id
           JOIN items i ON i.id=b.item_id
           JOIN agent_sessions s ON s.id=b.session_id
           JOIN chat_session_heads h ON h.project_id=b.project_id AND h.chat_id=b.chat_id
           JOIN session_runtime_epochs e ON e.project_id=b.project_id AND e.chat_id=b.chat_id
             AND e.session_id=b.session_id AND e.runtime_epoch=b.runtime_epoch
           WHERE b.item_id=?1 AND b.project_id=?2 AND b.chat_id=?3 AND b.session_id=?4
             AND b.runtime_epoch=?5 AND b.native_child_thread_id=?6
             AND o.project_id=b.project_id AND o.chat_id=b.chat_id
             AND o.session_id=b.session_id AND o.runtime_epoch=b.runtime_epoch
             AND o.parent_thread_id=?7 AND o.source='codexSubAgentActivity'
             AND i.project_id=b.project_id AND i.chat_id=b.chat_id
             AND i.session_id=b.session_id AND i.turn_id=b.turn_id
             AND i.runtime_epoch=b.runtime_epoch AND i.kind='collaboration'
             AND s.project_id=b.project_id AND s.chat_id=b.chat_id
             AND s.external_session_id=?7 AND s.state IN ('ready','active')
             AND h.session_id=b.session_id AND e.state='running'
             AND NOT EXISTS(SELECT 1 FROM session_runtime_epochs later
               WHERE later.session_id=b.session_id AND later.runtime_epoch>b.runtime_epoch)
         )",
        params![
            scope.item_id,
            scope.owner.project_id.as_str(),
            scope.owner.chat_id.as_str(),
            scope.owner.session_id.as_str(),
            epoch,
            scope.child_thread_id,
            scope.parent_thread_id,
        ],
        |row| row.get(0),
    )?;
    if found {
        Ok(())
    } else {
        Err(ChildTranscriptStoreError::Stale)
    }
}

pub(crate) fn persist_snapshot(
    connection: &mut Connection,
    scope: &ChildTranscriptScope,
    snapshot: &ChildTranscriptSnapshot,
    now_millis: i64,
) -> Result<(), ChildTranscriptStoreError> {
    if snapshot.child_thread_id != scope.child_thread_id
        || snapshot.parent_thread_id != scope.parent_thread_id
    {
        return Err(ChildTranscriptStoreError::Stale);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    verify_current_binding(&transaction, scope)?;
    let merged = match load_snapshot(&transaction, scope)? {
        Some(existing) => merge_snapshots(snapshot, &existing)?,
        None => snapshot.clone(),
    };
    validate_bounds(&merged)?;
    write_snapshot_in(&transaction, scope, &merged, now_millis, false)?;
    transaction.commit()?;
    Ok(())
}

/// The chat's child projection epoch: the newest `updated_at` among its child
/// transcripts. Live child changes stamp a value strictly above the current
/// epoch, so it advances on every change; a native re-read keeps its stamp,
/// so reading never wakes the reader again. It is an opaque, bounded
/// invalidation token, separate from the projection revision.
pub(crate) fn child_projection_epoch(
    connection: &Connection,
    project_id: &str,
    chat_id: &str,
) -> Result<u64, rusqlite::Error> {
    let epoch: i64 = connection.query_row(
        "SELECT COALESCE(MAX(updated_at),0) FROM acp_child_thread_transcripts
         WHERE project_id=?1 AND chat_id=?2",
        params![project_id, chat_id],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(epoch).unwrap_or(0))
}

fn write_snapshot_in(
    transaction: &rusqlite::Transaction<'_>,
    scope: &ChildTranscriptScope,
    snapshot: &ChildTranscriptSnapshot,
    now_millis: i64,
    advance_epoch: bool,
) -> Result<(), ChildTranscriptStoreError> {
    let current = child_projection_epoch(
        transaction,
        scope.owner.project_id.as_str(),
        scope.owner.chat_id.as_str(),
    )? as i64;
    let stamp = if advance_epoch {
        now_millis.max(current.saturating_add(1))
    } else {
        current
    };
    let encoded =
        serde_json::to_string(snapshot).map_err(|_| ChildTranscriptStoreError::Corrupt)?;
    if encoded.len() > MAX_SNAPSHOT_BYTES - 64 {
        return Err(ChildTranscriptStoreError::Capacity);
    }
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM acp_child_thread_transcripts WHERE native_child_thread_id=?1)",
        [scope.child_thread_id.as_str()],
        |row| row.get(0),
    )?;
    if !exists {
        let count: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM acp_child_thread_transcripts
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4",
            params![
                scope.owner.project_id.as_str(),
                scope.owner.chat_id.as_str(),
                scope.owner.session_id.as_str(),
                scope.owner.runtime_epoch.get() as i64,
            ],
            |row| row.get(0),
        )?;
        if count >= 32 {
            return Err(ChildTranscriptStoreError::Capacity);
        }
    }
    transaction.execute(
        "INSERT INTO acp_child_thread_transcripts
         (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,parent_thread_id,
          snapshot_json,snapshot_bytes,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(native_child_thread_id) DO UPDATE SET
           snapshot_json=excluded.snapshot_json,snapshot_bytes=excluded.snapshot_bytes,
           updated_at=CASE WHEN ?10 THEN excluded.updated_at ELSE updated_at END",
        params![
            scope.child_thread_id,
            scope.owner.project_id.as_str(),
            scope.owner.chat_id.as_str(),
            scope.owner.session_id.as_str(),
            scope.owner.runtime_epoch.get() as i64,
            scope.parent_thread_id,
            encoded,
            encoded.len() as i64,
            stamp,
            advance_epoch,
        ],
    )?;
    Ok(())
}

pub(crate) fn load_snapshot(
    connection: &Connection,
    scope: &ChildTranscriptScope,
) -> Result<Option<ChildTranscriptSnapshot>, ChildTranscriptStoreError> {
    use rusqlite::OptionalExtension;
    verify_current_binding(connection, scope)?;
    let encoded: Option<String> = connection
        .query_row(
            "SELECT snapshot_json FROM acp_child_thread_transcripts
             WHERE native_child_thread_id=?1 AND project_id=?2 AND chat_id=?3
               AND session_id=?4 AND runtime_epoch=?5 AND parent_thread_id=?6",
            params![
                scope.child_thread_id,
                scope.owner.project_id.as_str(),
                scope.owner.chat_id.as_str(),
                scope.owner.session_id.as_str(),
                scope.owner.runtime_epoch.get() as i64,
                scope.parent_thread_id,
            ],
            |row| row.get(0),
        )
        .optional()?;
    encoded
        .map(|encoded| {
            if encoded.len() > MAX_SNAPSHOT_BYTES {
                return Err(ChildTranscriptStoreError::Corrupt);
            }
            serde_json::from_str(&encoded).map_err(|_| ChildTranscriptStoreError::Corrupt)
        })
        .transpose()
}

pub(crate) fn load_earlier_snapshot(
    connection: &Connection,
    scope: &ChildTranscriptScope,
) -> Result<Option<ChildTranscriptSnapshot>, ChildTranscriptStoreError> {
    use rusqlite::OptionalExtension;
    let epoch = i64::try_from(scope.owner.runtime_epoch.get())
        .map_err(|_| ChildTranscriptStoreError::Stale)?;
    let encoded: Option<String> = connection
        .query_row(
            "SELECT t.snapshot_json FROM acp_child_thread_transcripts t
         JOIN acp_child_thread_bindings b ON b.native_child_thread_id=t.native_child_thread_id
         JOIN acp_child_thread_owners o ON o.native_child_thread_id=b.native_child_thread_id
         JOIN items i ON i.id=b.item_id
         JOIN agent_sessions s ON s.id=b.session_id
         JOIN chat_session_heads h ON h.project_id=b.project_id AND h.chat_id=b.chat_id
         JOIN session_runtime_epochs e ON e.project_id=b.project_id AND e.chat_id=b.chat_id
           AND e.session_id=b.session_id AND e.runtime_epoch=b.runtime_epoch
         WHERE b.item_id=?1 AND b.project_id=?2 AND b.chat_id=?3 AND b.session_id=?4
           AND b.runtime_epoch=?5 AND b.native_child_thread_id=?6
           AND o.project_id=b.project_id AND o.chat_id=b.chat_id
           AND o.session_id=b.session_id AND o.runtime_epoch=b.runtime_epoch
           AND o.parent_thread_id=?7 AND o.source='codexSubAgentActivity'
           AND i.project_id=b.project_id AND i.chat_id=b.chat_id
           AND i.session_id=b.session_id AND i.turn_id=b.turn_id
           AND i.runtime_epoch=b.runtime_epoch AND i.kind='collaboration'
           AND s.project_id=b.project_id AND s.chat_id=b.chat_id
           AND s.external_session_id=?7 AND s.state IN ('ready','active')
           AND h.session_id=b.session_id AND e.state='exited'
           AND t.project_id=b.project_id AND t.chat_id=b.chat_id
           AND t.session_id=b.session_id AND t.runtime_epoch=b.runtime_epoch
           AND t.parent_thread_id=?7
           AND NOT EXISTS(SELECT 1 FROM session_runtime_epochs later
             WHERE later.session_id=b.session_id AND later.runtime_epoch>b.runtime_epoch)",
            params![
                scope.item_id,
                scope.owner.project_id.as_str(),
                scope.owner.chat_id.as_str(),
                scope.owner.session_id.as_str(),
                epoch,
                scope.child_thread_id,
                scope.parent_thread_id,
            ],
            |row| row.get(0),
        )
        .optional()?;
    encoded
        .map(|encoded| {
            if encoded.len() > MAX_SNAPSHOT_BYTES {
                return Err(ChildTranscriptStoreError::Corrupt);
            }
            serde_json::from_str(&encoded).map_err(|_| ChildTranscriptStoreError::Corrupt)
        })
        .transpose()
}
