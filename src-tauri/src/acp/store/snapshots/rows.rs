use super::{FilePairCommand, ItemIngressCommand, StoreError};
use rusqlite::{OptionalExtension, Transaction, params};
use sage_acp_protocol::{
    contract::SageItemId,
    intent::{NormalizedPayload, SafeResourceLocation},
};
use sage_core::limits::LIMITS;

pub(super) struct Existing {
    pub id: String,
    pub payload: NormalizedPayload,
    pub payload_bytes: i64,
}

pub(super) fn lookup(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<Option<Existing>, StoreError> {
    if let NormalizedPayload::FileChangeDiff {
        resource: sage_acp_protocol::intent::SafeResourceLocation::Resource { resource },
        ..
    } = &command.payload
    {
        let owned: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM project_roots WHERE id=?1 AND project_id=?2)",
            params![resource.root_id.as_str(), command.project_id.as_str()],
            |row| row.get(0),
        )?;
        if !owned {
            return Err(StoreError::Conflict {
                resource: "filePairResource",
            });
        }
    }
    let mut statement = transaction.prepare(
        "SELECT id,project_id,chat_id,turn_id,payload_bytes,payload_digest,
         CASE WHEN length(CAST(payload_json AS BLOB))<=?4 THEN payload_json END
         FROM items WHERE session_id=?1 AND runtime_epoch=?2 AND external_item_id=?3
         AND kind='fileChangeDiff' LIMIT 2",
    )?;
    let mut rows = statement.query(params![
        command.session_id.as_str(),
        command.runtime_epoch.get() as i64,
        command.external_item_id.as_str(),
        LIMITS.wire.normalized_item_bytes as i64
    ])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    if row.get::<_, String>(1)? != command.project_id.as_str()
        || row.get::<_, String>(2)? != command.chat_id.as_str()
        || row.get::<_, String>(3)? != command.turn_id.as_str()
    {
        return Err(StoreError::Conflict {
            resource: "filePairOwner",
        });
    }
    let json: String = row.get(6)?;
    let payload_bytes = row.get::<_, i64>(4)?;
    if json.len() as i64 != payload_bytes
        || super::super::facts::hex_digest(json.as_bytes()) != row.get::<_, String>(5)?
    {
        return Err(StoreError::Corrupt("file metadata digest"));
    }
    let payload: NormalizedPayload =
        serde_json::from_str(&json).map_err(|_| StoreError::Corrupt("file metadata"))?;
    if !admissible(&payload, &command.payload) {
        return Err(StoreError::Conflict {
            resource: "filePairResource",
        });
    }
    let existing = Existing {
        id: row.get(0)?,
        payload,
        payload_bytes,
    };
    if rows.next()?.is_some() {
        return Err(StoreError::Conflict {
            resource: "itemIdentity",
        });
    }
    Ok(Some(existing))
}

fn admissible(existing: &NormalizedPayload, incoming: &NormalizedPayload) -> bool {
    let (
        NormalizedPayload::FileChangeDiff {
            resource: old,
            pair_available,
            snapshot_revision,
            ..
        },
        NormalizedPayload::FileChangeDiff { resource: next, .. },
    ) = (existing, incoming)
    else {
        return false;
    };
    if old == next {
        return true;
    }
    !*pair_available
        && *snapshot_revision == 0
        && matches!(old, SafeResourceLocation::Unavailable { .. })
        && matches!(next, SafeResourceLocation::Resource { .. })
}

pub(super) fn current_revision(
    transaction: &Transaction<'_>,
    existing: Option<&Existing>,
    digest: &str,
) -> Result<Option<u64>, StoreError> {
    let Some(existing) = existing else {
        return Ok(None);
    };
    let NormalizedPayload::FileChangeDiff {
        pair_available: true,
        content_digest,
        snapshot_revision,
        ..
    } = &existing.payload
    else {
        return Ok(None);
    };
    if *snapshot_revision == 0 || *snapshot_revision > i64::MAX as u64 {
        return Err(StoreError::Corrupt("file snapshot revision"));
    }
    if content_digest != digest {
        return Ok(None);
    }
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM acp_file_change_snapshots
         WHERE item_id=?1 AND snapshot_revision=?2 AND content_digest=?3)",
        params![existing.id, *snapshot_revision as i64, digest],
        |row| row.get(0),
    )?;
    if !present {
        return Err(StoreError::Corrupt("missing current file snapshot"));
    }
    Ok(Some(*snapshot_revision))
}

pub(super) fn next_revision(
    transaction: &Transaction<'_>,
    existing: Option<&Existing>,
) -> Result<u64, StoreError> {
    let Some(existing) = existing else {
        return Ok(1);
    };
    let max: i64 = transaction
        .query_row(
            "SELECT last_revision FROM acp_file_change_snapshot_sequences WHERE item_id=?1",
            [&existing.id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    max.checked_add(1)
        .and_then(|next| u64::try_from(next).ok())
        .ok_or(StoreError::CounterExhausted)
}

pub(super) fn turn_count(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<i64, StoreError> {
    Ok(transaction.query_row(
        "SELECT COUNT(*) FROM acp_file_change_snapshots WHERE project_id=?1 AND chat_id=?2
         AND session_id=?3 AND turn_id=?4 AND runtime_epoch=?5",
        params![
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64
        ],
        |row| row.get(0),
    )?)
}

pub(super) fn insert(
    transaction: &Transaction<'_>,
    item: &ItemIngressCommand,
    item_id: &SageItemId,
    pair: &FilePairCommand,
) -> Result<(), StoreError> {
    let NormalizedPayload::FileChangeDiff {
        resource: sage_acp_protocol::intent::SafeResourceLocation::Resource { resource },
        snapshot_revision,
        old_bytes,
        new_bytes,
        content_digest,
        pair_available: true,
        ..
    } = &item.payload
    else {
        return Err(StoreError::BatchShape);
    };
    transaction.execute(
        "INSERT INTO acp_file_change_snapshots(item_id,project_id,chat_id,session_id,turn_id,
         runtime_epoch,diff_ordinal,snapshot_revision,root_id,relative_path,old_blob,new_blob,
         old_bytes,new_bytes,content_digest,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,0,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            item_id.as_str(),
            item.project_id.as_str(),
            item.chat_id.as_str(),
            item.session_id.as_str(),
            item.turn_id.as_str(),
            item.runtime_epoch.get() as i64,
            *snapshot_revision as i64,
            resource.root_id.as_str(),
            resource.relative_path.as_str(),
            pair.old_text.as_bytes(),
            pair.new_text.as_bytes(),
            *old_bytes as i64,
            *new_bytes as i64,
            content_digest,
            pair.now_millis
        ],
    )?;
    let updated = transaction.execute(
        "INSERT INTO acp_file_change_snapshot_sequences(item_id,last_revision) VALUES(?1,?2)
         ON CONFLICT(item_id) DO UPDATE SET last_revision=excluded.last_revision
         WHERE last_revision < excluded.last_revision",
        params![item_id.as_str(), *snapshot_revision as i64],
    )?;
    if updated != 1 {
        return Err(StoreError::Corrupt("file snapshot sequence"));
    }
    Ok(())
}
