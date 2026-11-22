//! What each finished agent file change left on disk, so a file rewind can
//! tell whether a file still holds the agent's last write.
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sage_acp_protocol::intent::SafeResourceRef;

use super::{AdapterStoreOwner, StoreError, facts};

const KIND: &str = "agentWrite";
/// Writes recorded per Turn. A Turn's later writes go unrecorded, so a rewind
/// over them finds them unverified and refuses.
const WRITES_PER_TURN: i64 = 256;

/// Records one write unless the file's last write in this Turn left the same
/// bytes. Out of room, the write goes unrecorded: the batch still applies.
pub(crate) fn record(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    resource: &SafeResourceRef,
    digest: &[u8; 32],
    now_millis: i64,
) -> Result<bool, StoreError> {
    let (session, turn) = (owner.session_id.as_str(), owner.turn_id.as_str());
    let digest = hex(digest);
    let (root, path) = (resource.root_id.as_str(), resource.relative_path.as_str());
    let (last, count): (Option<String>, i64) = transaction.query_row(
        "SELECT (SELECT json_extract(payload_json,'$.d') FROM session_facts
           WHERE session_id=?1 AND turn_id=?2 AND kind=?3
             AND json_extract(payload_json,'$.r')=?4 AND json_extract(payload_json,'$.p')=?5
           ORDER BY sequence DESC LIMIT 1),
         (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND turn_id=?2 AND kind=?3)",
        params![session, turn, KIND, root, path],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if last.as_deref() == Some(digest.as_str()) || count >= WRITES_PER_TURN {
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

/// The bytes the agent's last write left in `resource`, over the turns still
/// in the conversation.
pub(crate) fn last_write(
    connection: &Connection,
    session: &str,
    resource: &SafeResourceRef,
) -> Result<Option<[u8; 32]>, StoreError> {
    let digest: Option<String> = connection
        .query_row(
            concat!(
                "SELECT json_extract(f.payload_json,'$.d') FROM session_facts f
                 WHERE f.session_id=?1 AND f.kind=?2
                   AND json_extract(f.payload_json,'$.r')=?3
                   AND json_extract(f.payload_json,'$.p')=?4 AND ",
                super::turn_undo::visible_turn!("f.session_id", "f.turn_id"),
                " ORDER BY f.sequence DESC LIMIT 1"
            ),
            params![
                session,
                KIND,
                resource.root_id.as_str(),
                resource.relative_path.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    digest.map(|digest| unhex(&digest)).transpose()
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
