#![allow(dead_code)] // Removed when the isolated packet receives central wiring.

mod activation_outcome;
mod activity_ingress;
mod adapter_batch;
mod cancel;
mod commands;
mod facts;
mod history;
mod history_activities;
mod history_cursor;
mod history_items;
mod item_ingress;
mod item_usage;
mod mutations;
mod paging;
mod permission_item;
mod projection_decode;
mod request_identity;
mod request_terminal;
mod requests;
mod residency;
mod send;
mod session_close;
mod session_duplicates;
mod sessions;
mod state;
mod terminal;
mod terminalize;

use serde::{Deserialize, Serialize};

use crate::acp::{
    contract::{
        ClientIntentId, EntityRevision, ProjectGeneration, ProjectionRevision, RuntimeEpoch,
        SageActivityId, SageAgentSessionId, SageChatId, SageItemId, SageProjectId,
        SageServerRequestId, SageTurnId,
    },
    intent::Prompt,
    lifecycle::{
        ActivityLifecycle, DeliveryState, ItemLifecycle, ServerRequestState, SessionState,
        ToolState, TurnState,
    },
};

type StartDuplicateRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
);

#[allow(unused_imports)]
pub(crate) use activation_outcome::{
    CompleteActivationCommand, FailActivationCommand, complete_activation, fail_activation,
};
#[allow(unused_imports)]
pub(crate) use activity_ingress::{
    ActivityIngressCommand, ActivityIngressMutation, apply_activity_ingress,
};
#[allow(unused_imports)]
pub(crate) use adapter_batch::{
    AdapterBatchMutation, AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner,
    AdapterTurnCursor, apply_adapter_inbound_batch,
};
#[allow(unused_imports)]
pub(crate) use cancel::begin_cancel;
#[allow(unused_imports)]
pub(crate) use commands::{lookup_active_turn, lookup_prepared_turn, prepare_turn};
pub(crate) use facts::next_projection_revision;
#[allow(unused_imports)]
pub(crate) use history::load_chat_page;
#[allow(unused_imports)]
pub(crate) use item_ingress::{ItemIngressCommand, ItemIngressMutation, apply_item_ingress};
pub(crate) use mutations::{StoreMutation, StoreMutationResult};
pub(crate) use paging::{RecoveryCandidate, load_recovery_candidates};
pub(crate) use request_terminal::apply_request_state;
#[allow(unused_imports)]
pub(crate) use requests::{DenyOnlyPermissionJournal, receive_permission_request};
#[allow(unused_imports)]
pub(crate) use residency::{ParkSessionCommand, park_session_after_zero_process};
#[allow(unused_imports)]
pub(crate) use send::{begin_send, parse_turn_state, record_send_observation};
#[allow(unused_imports)]
pub(crate) use session_close::{
    FinalizeCloseCommand, close_session, finalize_close_after_zero_process,
};
#[allow(unused_imports)]
pub(crate) use sessions::{
    ActivateSessionCommand, CloseSessionCommand, SessionMutation, StartSessionCommand,
    activate_session, start_session,
};
#[allow(unused_imports)]
pub(crate) use terminal::{
    apply_activity_terminal, apply_item_terminal, apply_session_terminal, apply_turn_terminal,
};
pub(crate) use terminalize::terminalize_recovery_epoch;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
    #[error("database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("durable history capacity reached for {resource}; existing history is preserved")]
    Capacity { resource: &'static str, limit: u64 },
    #[error("stale or foreign {resource} intent")]
    Conflict { resource: &'static str },
    #[error("invalid durable transition for {resource}")]
    InvalidTransition { resource: &'static str },
    #[error("conflicting terminal update for {resource}")]
    TerminalConflict { resource: &'static str },
    #[error("durable data is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("normalized payload exceeds the frozen limit")]
    PayloadTooLarge,
    #[error("journal transaction shape is unsupported")]
    BatchShape,
    #[error("SQLite commit outcome is unknown; authoritative bootstrap is required")]
    CommitUnknown,
    #[error("a monotonic SQLite counter is exhausted")]
    CounterExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrepareTurnCommand {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub prompt: Prompt,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedTurn {
    pub turn_id: SageTurnId,
    pub item_id: SageItemId,
    pub turn_revision: EntityRevision,
    pub projection_revision: ProjectionRevision,
    pub disposition: MutationDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MutationDisposition {
    Applied,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendObservation {
    ZeroBytes,
    PossibleBytes,
    CompleteBytes,
    Correlated,
    ProcessLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnMutationCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub expected_revision: EntityRevision,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BeginCancelCommand {
    pub turn: TurnMutationCommand,
    pub client_intent_id: ClientIntentId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnMutation {
    pub state: TurnState,
    pub delivery_state: DeliveryState,
    pub revision: EntityRevision,
    pub projection_revision: ProjectionRevision,
    pub disposition: MutationDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionTerminalCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub terminal: SessionState,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemTerminalCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub item_id: SageItemId,
    pub lifecycle: ItemLifecycle,
    pub tool_state: Option<ToolState>,
    pub payload_digest: String,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivityTerminalCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub activity_id: SageActivityId,
    pub lifecycle: ActivityLifecycle,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestStateCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub request_id: SageServerRequestId,
    pub state: ServerRequestState,
    pub now_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntityMutation {
    pub revision: EntityRevision,
    pub projection_revision: ProjectionRevision,
    pub disposition: MutationDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FactPayload {
    pub kind: String,
    pub entity_id: String,
    pub state: String,
}

/// A journal sink must commit the entire slice in one database transaction.
/// Implementations may not expose a partial prefix on error.
pub(crate) trait AcpStore: Send {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoreCommit {
    pub projection_revision: ProjectionRevision,
    pub results: Vec<StoreMutationResult>,
}

pub(crate) struct DatabaseAcpStore {
    database: crate::persistence::Database,
}

impl DatabaseAcpStore {
    pub(crate) fn new(database: crate::persistence::Database) -> Self {
        Self { database }
    }
}

impl AcpStore for DatabaseAcpStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let mut connection = self
            .database
            .write_connection()
            .map_err(|_| StoreError::Corrupt("storage recovery required"))?;
        commit_batch(&mut connection, batch)
    }
}

fn commit_batch(
    connection: &mut rusqlite::Connection,
    batch: &[StoreMutation],
) -> Result<StoreCommit, StoreError> {
    if batch.is_empty()
        || batch.len()
            > crate::acp::limits::LIMITS
                .queues
                .journal_facts_per_transaction as usize
    {
        return Err(StoreError::BatchShape);
    }
    validate_batch_shape(batch)?;
    if batch[0].is_standalone() {
        return mutations::standalone_commit(connection, &batch[0]);
    }
    let transaction = connection.transaction()?;
    let mut results = Vec::with_capacity(batch.len());
    let mut projection_revision = ProjectionRevision::ZERO;
    for mutation in batch {
        let result = match mutation {
            StoreMutation::LookupTurn(command) => {
                let prepared = lookup_prepared_turn(&transaction, command)?;
                projection_revision = prepared
                    .as_ref()
                    .map_or(command.expected_projection_revision, |value| {
                        value.projection_revision
                    });
                StoreMutationResult::Lookup(prepared)
            }
            StoreMutation::LookupActiveTurn(command) => {
                let turn = lookup_active_turn(&transaction, command)?;
                projection_revision = turn
                    .as_ref()
                    .map_or(ProjectionRevision::ZERO, |value| value.projection_revision);
                StoreMutationResult::ActiveTurnLookup(turn)
            }
            StoreMutation::PrepareTurn(command) => {
                let prepared = commands::prepare_turn_in(&transaction, command.clone())?;
                projection_revision = prepared.projection_revision;
                StoreMutationResult::Prepared(prepared)
            }
            StoreMutation::BeginSend(command) => {
                let turn = send::begin_send_in(&transaction, command.clone())?;
                projection_revision = turn.projection_revision;
                StoreMutationResult::Turn(turn)
            }
            StoreMutation::ObserveSend {
                command,
                observation,
            } => {
                let turn =
                    send::record_send_observation_in(&transaction, command.clone(), *observation)?;
                projection_revision = turn.projection_revision;
                StoreMutationResult::Turn(turn)
            }
            _ => unreachable!("standalone mutation dispatched before legacy transaction"),
        };
        results.push(result);
    }
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(StoreCommit {
        projection_revision,
        results,
    })
}

fn validate_batch_shape(batch: &[StoreMutation]) -> Result<(), StoreError> {
    if batch.iter().any(StoreMutation::is_standalone) {
        return if batch.len() == 1 {
            Ok(())
        } else {
            Err(StoreError::BatchShape)
        };
    }
    let first = mutation_shape(&batch[0]);
    let contains_lookup = first.2;
    for mutation in &batch[1..] {
        let shape = mutation_shape(mutation);
        if shape.0 != first.0
            || shape.1 != first.1
            || shape.2 != contains_lookup
            || (contains_lookup && !shape.2)
        {
            return Err(StoreError::BatchShape);
        }
    }
    Ok(())
}

fn mutation_shape(mutation: &StoreMutation) -> (&str, &str, bool) {
    match mutation {
        StoreMutation::LookupTurn(command) => {
            (command.project_id.as_str(), command.chat_id.as_str(), true)
        }
        StoreMutation::LookupActiveTurn(command) => {
            (command.project_id.as_str(), command.chat_id.as_str(), true)
        }
        StoreMutation::PrepareTurn(command) => {
            (command.project_id.as_str(), command.chat_id.as_str(), false)
        }
        StoreMutation::BeginSend(command) | StoreMutation::ObserveSend { command, .. } => {
            (command.project_id.as_str(), command.chat_id.as_str(), false)
        }
        _ => unreachable!("standalone mutations are rejected before legacy shape inspection"),
    }
}

#[cfg(test)]
mod adapter_batch_tests;
#[cfg(test)]
mod extended_tests;
#[cfg(test)]
mod ingress_tests;
#[cfg(test)]
mod journal_store_tests;
#[cfg(test)]
mod prepare_tests;
#[cfg(test)]
mod tests;
