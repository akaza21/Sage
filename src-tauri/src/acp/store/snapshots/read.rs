use crate::{
    acp::contract::SageRootId,
    acp_protocol::file_diff::{
        DIFF_SIDE_BYTES_MAX, DiffRef, SnapshotData, SnapshotReadError, pair_digest,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, params};

/// Check owners and lengths before copying either BLOB; SQLite's bounded
/// projection also rejects oversized stored values before decoding them.
pub(crate) fn read_snapshot(
    connection: &Connection,
    reference: &DiffRef,
    project_roots: &[SageRootId],
) -> Result<SnapshotData, SnapshotReadError> {
    reference.validate()?;
    if !project_roots
        .iter()
        .any(|root| root.as_str() == reference.root_id)
    {
        return Err(SnapshotReadError::Forbidden);
    }
    connection
        .query_row(
            "SELECT CASE WHEN length(old_blob)<=?3 THEN old_blob END,
         CASE WHEN length(new_blob)<=?3 THEN new_blob END,
         old_bytes,new_bytes,content_digest,snapshot_revision,root_id,relative_path,
         project_id,chat_id,session_id,turn_id,runtime_epoch,diff_ordinal
         FROM acp_file_change_snapshots WHERE item_id=?1 AND snapshot_revision=?2",
            params![
                reference.item_id,
                reference.snapshot_revision as i64,
                DIFF_SIDE_BYTES_MAX as i64
            ],
            |row| Ok(decode(row, reference)),
        )
        .optional()
        .map_err(|_| SnapshotReadError::Corrupt)?
        .unwrap_or(Err(SnapshotReadError::Unavailable))
}

fn decode(row: &Row<'_>, reference: &DiffRef) -> Result<SnapshotData, SnapshotReadError> {
    let text = |column| {
        row.get_ref(column)
            .map_err(|_| SnapshotReadError::Corrupt)?
            .as_str()
            .map_err(|_| SnapshotReadError::Corrupt)
    };
    let number = |column| {
        row.get::<_, i64>(column)
            .map_err(|_| SnapshotReadError::Corrupt)
    };
    for (column, expected) in [
        (6, &reference.root_id),
        (7, &reference.relative_path),
        (8, &reference.project_id),
        (9, &reference.chat_id),
        (10, &reference.session_id),
        (11, &reference.turn_id),
    ] {
        if text(column)? != expected {
            return Err(SnapshotReadError::Stale);
        }
    }
    if number(5)? != reference.snapshot_revision as i64
        || number(12)? != reference.runtime_epoch as i64
        || number(13)? != reference.diff_ordinal as i64
    {
        return Err(SnapshotReadError::Stale);
    }
    let old = row
        .get_ref(0)
        .map_err(|_| SnapshotReadError::Corrupt)?
        .as_blob()
        .map_err(|_| SnapshotReadError::Corrupt)?;
    let new = row
        .get_ref(1)
        .map_err(|_| SnapshotReadError::Corrupt)?
        .as_blob()
        .map_err(|_| SnapshotReadError::Corrupt)?;
    let old_bytes = number(2)?;
    let new_bytes = number(3)?;
    let digest = text(4)?;
    if old_bytes < 0
        || new_bytes < 0
        || old.len() > DIFF_SIDE_BYTES_MAX
        || new.len() > DIFF_SIDE_BYTES_MAX
        || old.len() as i64 != old_bytes
        || new.len() as i64 != new_bytes
        || digest != reference.content_digest
        || pair_digest(old, new) != digest
    {
        return Err(SnapshotReadError::Corrupt);
    }
    let old_text = std::str::from_utf8(old).map_err(|_| SnapshotReadError::Corrupt)?;
    let new_text = std::str::from_utf8(new).map_err(|_| SnapshotReadError::Corrupt)?;
    Ok(SnapshotData {
        old_text: old_text.to_owned(),
        new_text: new_text.to_owned(),
        old_bytes: old.len(),
        new_bytes: new.len(),
        snapshot_revision: reference.snapshot_revision,
        content_digest: digest.to_owned(),
    })
}
