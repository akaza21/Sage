//! The agent's latest write to each file, so a file rewind can tell whether a
//! file still holds it. One row per session and file, replaced on each write,
//! recorded only for sessions whose file rewinds read it.
use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sage_acp_protocol::intent::SafeResourceRef;
use sage_core::limits::LIMITS;

use super::{AdapterStoreOwner, StoreError};

/// Distinct files recorded per session. A file first written past it goes
/// unrecorded, so a rewind over it cannot verify it and keeps it.
const FILES_PER_SESSION: i64 = 4_096;
/// Recording stops once the session's facts reach this share of their limit,
/// the reserve activity rows keep too.
const HEADROOM_PERCENT: u64 = 90;

/// The agent's last write to each file, keyed by root id and relative path.
/// `None` for a write whose bytes went unread.
pub(crate) type LastWrites = HashMap<(String, String), Option<[u8; 32]>>;

/// Records the Turn's write as the file's latest. Out of room, a new file goes
/// unrecorded and a known one is marked unread, so it reads as unverified
/// rather than changed; the batch still applies.
pub(crate) fn record(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    resource: &SafeResourceRef,
    digest: Option<&[u8; 32]>,
    now_millis: i64,
) -> Result<bool, StoreError> {
    let session = owner.session_id.as_str();
    let (root, path) = (resource.root_id.as_str(), resource.relative_path.as_str());
    let used: i64 = transaction.query_row(
        "SELECT fact_count FROM acp_session_usage WHERE session_id=?1",
        [session],
        |row| row.get(0),
    )?;
    let reserved = used >= (LIMITS.durable.facts_per_session * HEADROOM_PERCENT / 100) as i64;
    let known: bool = transaction
        .query_row(
            "SELECT 1 FROM acp_agent_file_writes
             WHERE session_id=?1 AND root_id=?2 AND relative_path=?3",
            params![session, root, path],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if reserved && !known {
        return Ok(false);
    }
    // In the reserve the row is only updated, never added, and its bytes unread.
    let digest = digest.filter(|_| !reserved);
    // A new file only: the count runs over the session's primary-key prefix.
    if !known {
        let files: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM acp_agent_file_writes WHERE session_id=?1",
            [session],
            |row| row.get(0),
        )?;
        if files >= FILES_PER_SESSION {
            return Ok(false);
        }
    }
    let changed = transaction.execute(
        "INSERT INTO acp_agent_file_writes(session_id,root_id,relative_path,turn_id,digest,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(session_id,root_id,relative_path) DO UPDATE SET
           turn_id=excluded.turn_id, digest=excluded.digest, updated_at=excluded.updated_at
         WHERE turn_id IS NOT excluded.turn_id OR digest IS NOT excluded.digest",
        params![
            session,
            root,
            path,
            owner.turn_id.as_str(),
            digest.map(|digest| digest.as_slice()),
            now_millis
        ],
    )?;
    Ok(changed == 1)
}

/// Every file's latest agent write, from turns still in the conversation, in
/// one bounded query. A write from a cut turn is absent: nothing older is
/// kept, so that file cannot be verified.
pub(crate) fn last_writes(
    connection: &Connection,
    session: &str,
) -> Result<LastWrites, StoreError> {
    let mut statement = connection.prepare(concat!(
        "SELECT w.root_id, w.relative_path, w.digest FROM acp_agent_file_writes w
         WHERE w.session_id=?1 AND ",
        super::turn_undo::visible_turn!("w.session_id", "w.turn_id"),
        " LIMIT ?2"
    ))?;
    let rows = statement.query_map(params![session, FILES_PER_SESSION], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<Vec<u8>>>(2)?,
        ))
    })?;
    let mut writes = LastWrites::new();
    for row in rows {
        let (root, path, digest) = row?;
        let digest = digest
            .map(|digest| <[u8; 32]>::try_from(digest.as_slice()))
            .transpose()
            .map_err(|_| StoreError::Corrupt("agent write digest"))?;
        writes.insert((root, path), digest);
    }
    Ok(writes)
}

#[cfg(test)]
#[path = "agent_writes_tests.rs"]
mod tests;
