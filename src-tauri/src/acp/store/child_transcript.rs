use rusqlite::{Connection, TransactionBehavior, params};

use crate::acp::scheduler::RuntimeOwner;

use crate::acp::bindings::codex::{ChildTranscriptSnapshot, MAX_SNAPSHOT_BYTES};

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
    let encoded =
        serde_json::to_string(snapshot).map_err(|_| ChildTranscriptStoreError::Corrupt)?;
    if encoded.len() > MAX_SNAPSHOT_BYTES {
        return Err(ChildTranscriptStoreError::Corrupt);
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    verify_current_binding(&transaction, scope)?;
    transaction.execute(
        "INSERT INTO acp_child_thread_transcripts
         (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,parent_thread_id,
          snapshot_json,snapshot_bytes,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(native_child_thread_id) DO UPDATE SET
           snapshot_json=excluded.snapshot_json,snapshot_bytes=excluded.snapshot_bytes,
           updated_at=excluded.updated_at",
        params![
            scope.child_thread_id,
            scope.owner.project_id.as_str(),
            scope.owner.chat_id.as_str(),
            scope.owner.session_id.as_str(),
            scope.owner.runtime_epoch.get() as i64,
            scope.parent_thread_id,
            encoded,
            encoded.len() as i64,
            now_millis,
        ],
    )?;
    transaction.commit()?;
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

/// A stopped epoch can serve its latest snapshot, but this query confers no
/// write authority. It requires the same current session and exact binding.
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
