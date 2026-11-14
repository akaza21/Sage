//! Atomic, immutable FileChange snapshots. Pending metadata and its full pair
//! become one item update in the adapter transaction; no half-pair is published.

use super::{ItemIngressCommand, MutationDisposition, StoreError};
use rusqlite::Transaction;
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
    file_diff::{PairAdmission, admit_pair},
    intent::{ExternalId, NormalizedPayload},
};
use sage_core::limits::DIFFS_PER_TURN;

mod read;
mod rows;
pub(crate) use read::read_snapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilePairCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub external_item_id: ExternalId,
    pub old_text: String,
    pub new_text: String,
    pub now_millis: i64,
}

pub(super) fn apply_pair_in(
    transaction: &Transaction<'_>,
    item: &ItemIngressCommand,
    pair: &FilePairCommand,
) -> Result<bool, StoreError> {
    let NormalizedPayload::FileChangeDiff {
        resource,
        summary,
        additions,
        deletions,
        pair_available: true,
        snapshot_revision: 0,
        old_bytes,
        new_bytes,
        content_digest,
        operation,
        ..
    } = &item.payload
    else {
        return Err(StoreError::BatchShape);
    };
    let pending_json =
        serde_json::to_string(&item.payload).map_err(|_| StoreError::Corrupt("file metadata"))?;
    if pending_json.len() as u64 > sage_core::limits::LIMITS.wire.normalized_item_bytes
        || super::facts::hex_digest(pending_json.as_bytes()) != item.payload_digest
    {
        return Err(StoreError::Corrupt("normalized item digest"));
    }
    if *old_bytes != pair.old_text.len() as u64 || *new_bytes != pair.new_text.len() as u64 {
        return Err(StoreError::Corrupt("file pair lengths"));
    }
    let existing = rows::lookup(transaction, item)?;
    let mut command = item.clone();
    let summary_only = NormalizedPayload::file_summary_only(
        sage_acp_protocol::intent::FileChangeFacts {
            resource: resource.clone(),
            summary: summary.clone(),
            additions: *additions,
            deletions: *deletions,
            operation: *operation,
        },
        true,
    );
    let PairAdmission::Admitted {
        old_bytes,
        new_bytes,
        digest,
    } = admit_pair(&pair.old_text, &pair.new_text)
    else {
        command.payload = summary_only;
        return persist_item(transaction, command);
    };
    if &digest != content_digest {
        return Err(StoreError::Corrupt("file pair digest"));
    }
    let retained = rows::current_revision(transaction, existing.as_ref(), &digest)?;
    if retained.is_none() && rows::turn_count(transaction, item)? >= DIFFS_PER_TURN {
        command.payload = summary_only;
        return persist_item(transaction, command);
    }
    let revision = match retained {
        Some(revision) => revision,
        None => rows::next_revision(transaction, existing.as_ref())?,
    };
    command.payload = NormalizedPayload::file_admitted(
        sage_acp_protocol::intent::FileChangeFacts {
            resource: resource.clone(),
            summary: summary.clone(),
            additions: *additions,
            deletions: *deletions,
            operation: *operation,
        },
        revision,
        (old_bytes as u64, new_bytes as u64),
        digest,
    );
    set_digest(&mut command)?;
    let pair_bytes = if retained.is_some() {
        0
    } else {
        (old_bytes + new_bytes) as i64
    };
    let old_payload_bytes = existing.as_ref().map_or(0, |item| item.payload_bytes);
    let metadata_bytes = serde_json::to_string(&command.payload)
        .map_err(|_| StoreError::Corrupt("file metadata"))?
        .len() as i64;
    let delta = metadata_bytes
        .checked_sub(old_payload_bytes)
        .and_then(|delta| delta.checked_add(pair_bytes))
        .ok_or(StoreError::CounterExhausted)?;

    // Only capacity overflow degrades to summary-only. Any stale owner,
    // malformed input or failed write rolls back the entire outer batch.
    transaction.execute_batch("SAVEPOINT acp_file_pair_admission")?;
    let attempt = (|| {
        super::item_usage::ensure_capacity(
            transaction,
            &command,
            i64::from(existing.is_none()),
            delta.max(0),
        )?;
        // Reserve shared bytes before fact insertion so facts see the full
        // charge. Reservation, item, blob and publication share one transaction.
        if pair_bytes > 0 {
            super::item_usage::update(transaction, &command, 0, pair_bytes)?;
        }
        let mutation = super::item_ingress::apply_file_change_ingress_in(transaction, &command)?;
        if retained.is_none() {
            rows::insert(transaction, &command, &mutation.item_id, pair)?;
        }
        Ok(mutation.entity.disposition == MutationDisposition::Applied)
    })();
    match attempt {
        Ok(applied) => {
            transaction.execute_batch("RELEASE acp_file_pair_admission")?;
            Ok(applied)
        }
        Err(StoreError::Capacity { .. }) => {
            transaction.execute_batch(
                "ROLLBACK TO acp_file_pair_admission; RELEASE acp_file_pair_admission",
            )?;
            command.payload = summary_only;
            persist_item(transaction, command)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn apply_summary_in(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
) -> Result<bool, StoreError> {
    if !matches!(&command.payload, NormalizedPayload::FileChangeDiff {
        pair_available: false, snapshot_revision: 0, old_bytes: 0, new_bytes: 0,
        content_digest, ..
    } if content_digest.is_empty())
    {
        return Err(StoreError::BatchShape);
    }
    rows::lookup(transaction, command)?;
    // Ordinary summary ingress retains its original digest validation.
    Ok(
        super::item_ingress::apply_file_change_ingress_in(transaction, command)?
            .entity
            .disposition
            == MutationDisposition::Applied,
    )
}

fn persist_item(
    transaction: &Transaction<'_>,
    mut command: ItemIngressCommand,
) -> Result<bool, StoreError> {
    set_digest(&mut command)?;
    Ok(
        super::item_ingress::apply_file_change_ingress_in(transaction, &command)?
            .entity
            .disposition
            == MutationDisposition::Applied,
    )
}

fn set_digest(command: &mut ItemIngressCommand) -> Result<(), StoreError> {
    let json = serde_json::to_string(&command.payload)
        .map_err(|_| StoreError::Corrupt("file metadata"))?;
    command.payload_digest = super::facts::hex_digest(json.as_bytes());
    Ok(())
}
