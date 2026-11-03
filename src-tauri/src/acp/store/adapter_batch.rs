use rusqlite::Transaction;

use crate::acp::{
    contract::{
        EntityRevision, ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    intent::{ActivityProvenanceV1, ExternalId},
    lifecycle::{ActivityLifecycle, DeliveryState, TurnState, TurnTerminalReason},
    projection::ActivityRole,
    runtime::permissions::PermissionIntent,
};

use super::snapshots::FilePairCommand;
use super::{ItemIngressCommand, MutationDisposition, RequestStateCommand, StoreError};

mod activity_item;
mod child_threads;
mod file_pairs;
mod scope;
mod terminal_reason;
mod turns;
mod usage;

pub(crate) use terminal_reason::TurnTerminalReasonCommand;

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
pub(crate) struct TurnUsageSnapshotCommand {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub context_window: Option<u32>,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildLiveItemCommand {
    pub child_thread_id: ExternalId,
    pub child_turn_id: ExternalId,
    pub item_id: ExternalId,
    pub role: Option<crate::acp::bindings::codex::ChildTextRole>,
    pub lifecycle: crate::acp::lifecycle::ItemLifecycle,
    pub tool_state: Option<crate::acp::lifecycle::ToolState>,
    pub payload: crate::acp::intent::NormalizedPayload,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdapterInboundEvent {
    Turn {
        state: TurnState,
        now_millis: i64,
    },
    Item(ItemIngressCommand),
    ChildThreadItem {
        item: ItemIngressCommand,
        child_thread_id: ExternalId,
    },
    ChildLiveItem(ChildLiveItemCommand),
    ChildTurnStatus {
        child_thread_id: ExternalId,
        child_turn_id: ExternalId,
        status: TurnState,
        now_millis: i64,
    },
    UsageSnapshot(TurnUsageSnapshotCommand),
    TerminalReason(TurnTerminalReasonCommand),
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
    PermissionResolved(RequestStateCommand),
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
    /// The chat's child projection epoch when this batch advanced it.
    pub child_projection_epoch: Option<u64>,
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
            child_projection_epoch: None,
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
        scope::preflight(&command)?;
        Ok(command)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Read-only view of the store-verified owner of this batch.
    pub(crate) fn owner(&self) -> &AdapterStoreOwner {
        &self.owner
    }

    /// Whether any event in this batch moves the owner Turn to a terminal state.
    pub(crate) fn ends_turn(&self) -> bool {
        self.events.iter().any(
            |event| matches!(event, AdapterInboundEvent::Turn { state, .. } if state.is_terminal()),
        )
    }
}

#[cfg(test)]
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
    scope::preflight(command)?;
    let transaction = connection.transaction()?;
    scope::verify_owner(&transaction, &command.owner)?;
    let initial = scope::current_projection(&transaction, &command.owner)?;
    let child_epoch = || {
        super::child_transcript::child_projection_epoch(
            &transaction,
            command.owner.project_id.as_str(),
            command.owner.chat_id.as_str(),
        )
    };
    let has_child_changes = command.events.iter().any(|event| {
        matches!(
            event,
            AdapterInboundEvent::ChildLiveItem(_) | AdapterInboundEvent::ChildTurnStatus { .. }
        )
    });
    let initial_child_epoch = if has_child_changes {
        Some(child_epoch()?)
    } else {
        None
    };
    let user_cancelled = contains_user_cancel_terminal(&command.events)
        && scope::has_durable_user_cancel_intent(&transaction, &command.owner)?;
    let mut applied = file_pairs::apply_events(&transaction, &command.owner, &command.events)?;
    let turn_cursor = turns::cursor(&transaction, &command.owner)?;
    if user_cancelled && user_cancel_terminal(turn_cursor.state()) {
        applied |= terminal_reason::apply(
            &transaction,
            &command.owner,
            &TurnTerminalReasonCommand {
                reason: TurnTerminalReason::CancelledByUser,
                message: None,
                remediation: None,
                now_millis: now_millis(&command.events),
            },
        )?;
    }
    let projection = turn_cursor.projection_revision();
    let child_projection_epoch = match initial_child_epoch {
        Some(before) => Some(child_epoch()?).filter(|after| *after != before),
        None => None,
    };
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
        child_projection_epoch,
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
            scope::verify_item_scope(owner, command)?;
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
        AdapterInboundEvent::ChildThreadItem {
            item,
            child_thread_id,
        } => child_threads::apply(transaction, owner, item, child_thread_id),
        AdapterInboundEvent::ChildLiveItem(command) => {
            super::child_transcript::persist_live_item_in(transaction, owner, command)
        }
        AdapterInboundEvent::ChildTurnStatus {
            child_thread_id,
            child_turn_id,
            status,
            now_millis,
        } => super::child_transcript::persist_turn_status_in(
            transaction,
            owner,
            child_thread_id,
            child_turn_id,
            *status,
            *now_millis,
        ),
        AdapterInboundEvent::UsageSnapshot(command) => usage::apply(transaction, owner, command),
        AdapterInboundEvent::TerminalReason(command) => {
            terminal_reason::apply(transaction, owner, command)
        }
        AdapterInboundEvent::FilePair(_) => Err(StoreError::BatchShape),
        event @ AdapterInboundEvent::ActivityItem(_) => {
            activity_item::apply(transaction, owner, event)
        }
        AdapterInboundEvent::PermissionRequest { intent, now_millis } => {
            scope::verify_permission_scope(owner, intent)?;
            let received =
                super::requests::receive_permission_request_in(transaction, intent, *now_millis)?;
            Ok(received.disposition == MutationDisposition::Applied)
        }
        AdapterInboundEvent::PermissionDeny { intent, now_millis } => {
            scope::verify_permission_scope(owner, intent)?;
            let received =
                super::requests::receive_permission_request_in(transaction, intent, *now_millis)?;
            let declined = super::requests::commit_decline_in(transaction, intent, *now_millis)?;
            Ok(received.disposition == MutationDisposition::Applied
                || declined == crate::acp::runtime::permissions::DeclineCommit::Applied)
        }
        AdapterInboundEvent::PermissionResolved(command) => {
            scope::verify_request_scope(owner, command)?;
            let resolved =
                super::request_terminal::apply_harness_resolution_in(transaction, command)?;
            Ok(resolved.disposition == MutationDisposition::Applied)
        }
    }
}

fn user_cancel_terminal(state: TurnState) -> bool {
    matches!(state, TurnState::Cancelled | TurnState::Interrupted)
}

fn contains_user_cancel_terminal(events: &[AdapterInboundEvent]) -> bool {
    events.iter().any(|event| {
        matches!(event, AdapterInboundEvent::Turn { state, .. } if user_cancel_terminal(*state))
    })
}

fn now_millis(events: &[AdapterInboundEvent]) -> i64 {
    events
        .iter()
        .filter_map(|event| match event {
            AdapterInboundEvent::Turn { state, now_millis } if user_cancel_terminal(*state) => {
                Some(*now_millis)
            }
            _ => None,
        })
        .max()
        .unwrap_or_default()
}
