//! What each finished agent file change left on disk, so a file rewind can
//! tell whether a file still holds the agent's last write. Recorded only for
//! sessions whose file rewinds read it.
use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sage_acp_protocol::intent::SafeResourceRef;

use super::{AdapterStoreOwner, StoreError, facts};

const KIND: &str = "agentWrite";
/// Write records per session. A session at the cap records no more, and its
/// file rewinds treat every file as unverified.
pub(crate) const SESSION_WRITES: i64 = 2_048;

/// The agent's last write to each file, keyed by root id and relative path.
/// `None` for a write whose bytes went unread.
pub(crate) type LastWrites = HashMap<(String, String), Option<[u8; 32]>>;

/// Records one write unless this file's last write in this Turn left the same
/// bytes. Out of room, the write goes unrecorded: the batch still applies.
pub(crate) fn record(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    resource: &SafeResourceRef,
    digest: Option<&[u8; 32]>,
    now_millis: i64,
) -> Result<bool, StoreError> {
    let (session, turn) = (owner.session_id.as_str(), owner.turn_id.as_str());
    let digest = digest.map(hex);
    let (root, path) = (resource.root_id.as_str(), resource.relative_path.as_str());
    // The file's last write in this Turn, as (digest) rows: none, or one.
    let last: Option<Option<String>> = transaction
        .query_row(
            "SELECT json_extract(payload_json,'$.d') FROM session_facts
             WHERE session_id=?1 AND turn_id=?2 AND kind=?3
               AND json_extract(payload_json,'$.r')=?4 AND json_extract(payload_json,'$.p')=?5
             ORDER BY sequence DESC LIMIT 1",
            params![session, turn, KIND, root, path],
            |row| row.get(0),
        )
        .optional()?;
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind=?2",
        params![session, KIND],
        |row| row.get(0),
    )?;
    if last == Some(digest.clone()) || count >= SESSION_WRITES {
        return Ok(false);
    }
    let payload = serde_json::json!({"r": root, "p": path, "d": digest}).to_string();
    let revision = facts::next_projection_revision(
        transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
    )?;
    match facts::append_fact_payload(
        transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        session,
        Some(turn),
        KIND,
        &payload,
        revision,
        now_millis,
    ) {
        Ok(()) => Ok(true),
        Err(StoreError::Capacity { .. } | StoreError::PayloadTooLarge) => Ok(false),
        Err(error) => Err(error),
    }
}

/// The agent's last write to every file, over the turns still in the
/// conversation, in one bounded query. `None` when the session is at its cap,
/// so later writes may have gone unrecorded.
pub(crate) fn last_writes(
    connection: &Connection,
    session: &str,
) -> Result<Option<LastWrites>, StoreError> {
    let total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind=?2",
        params![session, KIND],
        |row| row.get(0),
    )?;
    if total >= SESSION_WRITES {
        return Ok(None);
    }
    let mut statement = connection.prepare(concat!(
        "SELECT json_extract(f.payload_json,'$.r'), json_extract(f.payload_json,'$.p'),
           json_extract(f.payload_json,'$.d') FROM session_facts f
         WHERE f.session_id=?1 AND f.kind=?2 AND ",
        super::turn_undo::visible_turn!("f.session_id", "f.turn_id"),
        " ORDER BY f.sequence DESC LIMIT ?3"
    ))?;
    let rows = statement.query_map(params![session, KIND, SESSION_WRITES], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut writes = LastWrites::new();
    for row in rows {
        let (root, path, digest) = row?;
        if let std::collections::hash_map::Entry::Vacant(entry) = writes.entry((root, path)) {
            entry.insert(digest.as_deref().map(unhex).transpose()?);
        }
    }
    Ok(Some(writes))
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<[u8; 32], StoreError> {
    let mut bytes = [0_u8; 32];
    if text.len() != 64 || !text.is_ascii() {
        return Err(StoreError::Corrupt("agent write digest"));
    }
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| StoreError::Corrupt("agent write digest"))?;
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "agent_writes_tests.rs"]
mod tests;
