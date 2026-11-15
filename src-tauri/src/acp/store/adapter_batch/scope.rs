use rusqlite::{OptionalExtension, Transaction, params};

use sage_acp_protocol::{
    contract::{ProjectionRevision, RuntimeEpoch},
    permissions::PermissionIntent,
};
use sage_core::limits::LIMITS;

use super::super::snapshots::FilePairCommand;
use super::super::{ItemIngressCommand, RequestStateCommand, StoreError};
use super::{AdapterInboundBatchCommand, AdapterStoreOwner};

pub(super) fn preflight(command: &AdapterInboundBatchCommand) -> Result<(), StoreError> {
    if command.events.is_empty()
        || command.events.len() > LIMITS.queues.journal_facts_per_transaction as usize
        || command.actual_bytes == 0
        || command.actual_bytes > LIMITS.queues.journal_bytes_per_transaction as usize
        || command.retained_bytes == 0
        || command.retained_bytes > LIMITS.queues.journal_bytes_per_transaction as usize
        || command.owner.runtime_epoch == RuntimeEpoch::ZERO
        || !valid_digest(&command.owner.binding_digest)
    {
        return Err(StoreError::BatchShape);
    }
    Ok(())
}

pub(super) fn verify_owner(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
) -> Result<(), StoreError> {
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns t
         JOIN agent_sessions s ON s.id=t.session_id
         JOIN session_runtime_epochs e ON e.session_id=t.session_id
           AND e.runtime_epoch=t.runtime_epoch
         WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4
           AND t.runtime_epoch=?5 AND s.binding_digest=?6 AND s.external_session_id=?7
           AND s.state IN ('ready','active') AND s.residency='hot' AND e.state='running')",
        params![
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            owner.turn_id.as_str(),
            owner.runtime_epoch.get() as i64,
            owner.binding_digest,
            owner.external_session_id.as_str()
        ],
        |row| row.get(0),
    )?;
    if valid {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "adapterBatchOwner",
        })
    }
}

pub(super) fn verify_item_scope(
    owner: &AdapterStoreOwner,
    command: &ItemIngressCommand,
) -> Result<(), StoreError> {
    if command.project_id == owner.project_id
        && command.chat_id == owner.chat_id
        && command.session_id == owner.session_id
        && command.turn_id == owner.turn_id
        && command.runtime_epoch == owner.runtime_epoch
    {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "adapterItemOwner",
        })
    }
}

pub(super) fn verify_file_pair_scope(
    owner: &AdapterStoreOwner,
    command: &FilePairCommand,
) -> Result<(), StoreError> {
    if command.project_id == owner.project_id
        && command.chat_id == owner.chat_id
        && command.session_id == owner.session_id
        && command.turn_id == owner.turn_id
        && command.runtime_epoch == owner.runtime_epoch
    {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "adapterItemOwner",
        })
    }
}

pub(super) fn verify_permission_scope(
    owner: &AdapterStoreOwner,
    intent: &PermissionIntent,
) -> Result<(), StoreError> {
    if intent.project_id == owner.project_id
        && intent.chat_id == owner.chat_id
        && intent.session_id == owner.session_id
        && intent.turn_id == owner.turn_id
        && intent.runtime_epoch == owner.runtime_epoch
        && intent.binding_digest == owner.binding_digest
    {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "adapterPermissionOwner",
        })
    }
}

pub(super) fn verify_request_scope(
    owner: &AdapterStoreOwner,
    command: &RequestStateCommand,
) -> Result<(), StoreError> {
    if command.project_id == owner.project_id
        && command.chat_id == owner.chat_id
        && command.session_id == owner.session_id
        && command.turn_id == owner.turn_id
        && command.runtime_epoch == owner.runtime_epoch
    {
        Ok(())
    } else {
        Err(StoreError::Conflict {
            resource: "adapterPermissionOwner",
        })
    }
}

pub(super) fn current_projection(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
) -> Result<ProjectionRevision, StoreError> {
    let value: i64 = transaction
        .query_row(
            "SELECT projection_revision FROM agent_sessions
             WHERE project_id=?1 AND chat_id=?2 AND id=?3",
            params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "adapterBatchOwner",
        })?;
    super::super::facts::projection_from_i64(value)
}

pub(super) fn has_durable_user_cancel_intent(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
) -> Result<bool, StoreError> {
    transaction
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM turns
                WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4 AND id=?5
                  AND state='cancelling'
                  AND EXISTS(
                    SELECT 1 FROM session_facts
                    WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?5
                      AND kind='turnCancelIntent'
                  )
            )",
            params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
                owner.turn_id.as_str(),
            ],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
