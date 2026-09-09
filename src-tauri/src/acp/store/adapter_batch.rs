use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    contract::{
        EntityRevision, ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    intent::{ActivityProvenanceV1, ExternalId},
    lifecycle::{ActivityLifecycle, DeliveryState, TurnState},
    limits::LIMITS,
    projection::ActivityRole,
    runtime::permissions::PermissionIntent,
};

use super::snapshots::FilePairCommand;
use super::{ItemIngressCommand, MutationDisposition, StoreError};

mod activity_item;
mod file_pairs;
mod turns;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterStoreOwner {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub binding_digest: String,
    pub external_session_id: ExternalId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterActivityItemCommand {
    pub external_activity_id: ExternalId,
    pub activity_kind: RequiredText<128>,
    pub parent_external_activity_id: Option<ExternalId>,
    pub parent_external_item_id: ExternalId,
    pub role: ActivityRole,
    pub activity_lifecycle: ActivityLifecycle,
    pub depth: u8,
    pub observed_sequence: u64,
    pub provenance: ActivityProvenanceV1,
    pub provenance_digest: String,
    pub item: ItemIngressCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdapterInboundEvent {
    Turn {
        state: TurnState,
        now_millis: i64,
    },
    Item(ItemIngressCommand),
    /// Bounded old/new bytes for the `FileChangeDiff` item with the same
    /// owner and external key. Applied in the same transaction as its item.
    FilePair(FilePairCommand),
    ActivityItem(Box<AdapterActivityItemCommand>),
    PermissionRequest {
        intent: PermissionIntent,
        now_millis: i64,
    },
    PermissionDeny {
        intent: PermissionIntent,
        now_millis: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterInboundBatchCommand {
    owner: AdapterStoreOwner,
    actual_bytes: usize,
    retained_bytes: usize,
    events: Vec<AdapterInboundEvent>,
}

/// Store-minted cursor proving the exact durable Turn outcome of one adapter
/// batch. Private fields prevent production runtime code from forging proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterTurnCursor {
    turn_id: SageTurnId,
    state: TurnState,
    delivery_state: DeliveryState,
    revision: EntityRevision,
    projection_revision: ProjectionRevision,
}

impl AdapterTurnCursor {
    pub(crate) fn turn_id(&self) -> &SageTurnId {
        &self.turn_id
    }

    pub(crate) fn state(&self) -> TurnState {
        self.state
    }

    pub(crate) fn delivery_state(&self) -> DeliveryState {
        self.delivery_state
    }

    pub(crate) fn revision(&self) -> EntityRevision {
        self.revision
    }

    pub(crate) fn projection_revision(&self) -> ProjectionRevision {
        self.projection_revision
    }

    #[cfg(test)]
    pub(crate) fn test_only(
        turn_id: SageTurnId,
        state: TurnState,
        delivery_state: DeliveryState,
        revision: EntityRevision,
        projection_revision: ProjectionRevision,
    ) -> Self {
        Self {
            turn_id,
            state,
            delivery_state,
            revision,
            projection_revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterBatchMutation {
    pub projection_revision: ProjectionRevision,
    pub disposition: MutationDisposition,
    turn_cursor: AdapterTurnCursor,
}

impl AdapterBatchMutation {
    pub(crate) fn turn_cursor(&self) -> &AdapterTurnCursor {
        &self.turn_cursor
    }

    pub(crate) fn into_turn_cursor(self) -> AdapterTurnCursor {
        self.turn_cursor
    }

    #[cfg(test)]
    pub(crate) fn test_only(
        projection_revision: ProjectionRevision,
        disposition: MutationDisposition,
        turn_cursor: AdapterTurnCursor,
    ) -> Self {
        Self {
            projection_revision,
            disposition,
            turn_cursor,
        }
    }
}

impl AdapterInboundBatchCommand {
    pub(crate) fn new(
        owner: AdapterStoreOwner,
        actual_bytes: usize,
        retained_bytes: usize,
        events: Vec<AdapterInboundEvent>,
    ) -> Result<Self, StoreError> {
        let command = Self {
            owner,
            actual_bytes,
            retained_bytes,
            events,
        };
        preflight(&command)?;
        Ok(command)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

pub(crate) fn apply_adapter_inbound_batch(
    connection: &mut rusqlite::Connection,
    command: AdapterInboundBatchCommand,
) -> Result<AdapterBatchMutation, StoreError> {
    apply_adapter_inbound_batch_ref(connection, &command)
}

pub(super) fn apply_adapter_inbound_batch_ref(
    connection: &mut rusqlite::Connection,
    command: &AdapterInboundBatchCommand,
) -> Result<AdapterBatchMutation, StoreError> {
    preflight(command)?;
    let transaction = connection.transaction()?;
    verify_owner(&transaction, &command.owner)?;
    let initial = current_projection(&transaction, &command.owner)?;
    let applied = file_pairs::apply_events(&transaction, &command.owner, &command.events)?;
    let turn_cursor = turns::cursor(&transaction, &command.owner)?;
    let projection = turn_cursor.projection_revision();
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(AdapterBatchMutation {
        projection_revision: projection,
        disposition: if applied || projection != initial {
            MutationDisposition::Applied
        } else {
            MutationDisposition::Duplicate
        },
        turn_cursor,
    })
}

fn apply_event(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    event: &AdapterInboundEvent,
) -> Result<bool, StoreError> {
    match event {
        AdapterInboundEvent::Turn { state, now_millis } => {
            Ok(
                turns::apply(transaction, owner, *state, *now_millis)?.disposition
                    == MutationDisposition::Applied,
            )
        }
        AdapterInboundEvent::Item(command) => {
            verify_item_scope(owner, command)?;
            if matches!(
                command.payload,
                crate::acp::intent::NormalizedPayload::FileChangeDiff { .. }
            ) {
                return super::snapshots::apply_summary_in(transaction, command);
            }
            Ok(
                super::item_ingress::apply_item_ingress_in(transaction, command)?
                    .entity
                    .disposition
                    == MutationDisposition::Applied,
            )
        }
        AdapterInboundEvent::FilePair(_) => Err(StoreError::BatchShape),
        event @ AdapterInboundEvent::ActivityItem(_) => {
            activity_item::apply(transaction, owner, event)
        }
        AdapterInboundEvent::PermissionRequest { intent, now_millis } => {
            verify_permission_scope(owner, intent)?;
            let received =
                super::requests::receive_permission_request_in(transaction, intent, *now_millis)?;
            Ok(received.disposition == MutationDisposition::Applied)
        }
        AdapterInboundEvent::PermissionDeny { intent, now_millis } => {
            verify_permission_scope(owner, intent)?;
            let received =
                super::requests::receive_permission_request_in(transaction, intent, *now_millis)?;
            let declined = super::requests::commit_decline_in(transaction, intent, *now_millis)?;
            Ok(received.disposition == MutationDisposition::Applied
                || declined == crate::acp::runtime::permissions::DeclineCommit::Applied)
        }
    }
}

fn preflight(command: &AdapterInboundBatchCommand) -> Result<(), StoreError> {
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

fn verify_owner(
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

fn verify_file_pair_scope(
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

fn verify_permission_scope(
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

fn current_projection(
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
    super::facts::projection_from_i64(value)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
