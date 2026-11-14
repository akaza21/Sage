mod activation_outcome;
mod activity_ingress;
mod adapter_batch;
pub(crate) mod browser_operations;
#[cfg(test)]
mod browser_permission_tests;
mod cancel;
pub(crate) mod chat_fork;
pub(crate) mod child_transcript;
mod commands;
mod database_store;
pub(crate) mod edit_disposition;
mod facts;
mod history;
mod history_activities;
mod history_cursor;
mod history_items;
mod history_sessions;
mod item_ingress;
mod item_usage;
mod mutations;
pub(crate) mod native_import;
mod paging;
mod permission_item;
mod projection_decode;
mod prompt_images;
pub(crate) use projection_decode::decode_payload;
mod request_identity;
mod request_terminal;
mod requests;
mod residency;
mod send;
mod session_close;
mod session_duplicates;
mod session_head;
mod session_restart;
mod session_resume;
pub(crate) mod session_surface;
mod sessions;
mod snapshots;
mod state;
mod terminal;
mod terminal_reason;
mod terminalize;
mod turn_activity_facts;
pub(crate) mod turn_undo;

use serde::{Deserialize, Serialize};

use crate::acp::transcript_quote::Prompt;
#[cfg(test)]
use sage_acp_protocol::lifecycle::SessionState;
use sage_acp_protocol::{
    contract::{
        ClientIntentId, EntityRevision, ProjectGeneration, ProjectionRevision, RuntimeEpoch,
        SageAgentSessionId, SageChatId, SageItemId, SageProjectId, SageServerRequestId, SageTurnId,
    },
    lifecycle::{DeliveryState, ServerRequestState, TurnState},
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

pub(crate) use activation_outcome::{
    AbortActivationCommand, ActivationAbortCategory, CompleteActivationCommand,
    FailActivationCommand, abort_activation, complete_activation, fail_activation,
};
pub(crate) use activity_ingress::{
    ActivityIngressCommand, ActivityIngressMutation, apply_activity_ingress,
};
#[cfg(test)]
pub(crate) use adapter_batch::apply_adapter_inbound_batch;
pub(crate) use adapter_batch::{
    AdapterActivityItemCommand, AdapterBatchMutation, AdapterInboundBatchCommand,
    AdapterInboundEvent, AdapterStoreOwner, AdapterTurnCursor, ChildLiveItemCommand,
    TurnTerminalReasonCommand, TurnUsageSnapshotCommand,
};
pub(crate) use cancel::{begin_cancel, turn_is_terminal};
#[cfg(test)]
pub(crate) use commands::prepare_turn;
pub(crate) use commands::{lookup_active_turn, lookup_prepared_turn};
#[cfg(test)]
pub(crate) use facts::hex_digest;
pub(crate) use facts::next_projection_revision;
pub(crate) use history::load_chat_page_reserving;

pub(crate) fn load_chat_page(
    connection: &rusqlite::Connection,
    project_id: sage_acp_protocol::contract::SageProjectId,
    chat_id: sage_acp_protocol::contract::SageChatId,
    cursor: Option<sage_acp_protocol::intent::PageCursor>,
) -> Result<crate::acp::projection::AcpChatPage, StoreError> {
    load_chat_page_reserving(connection, project_id, chat_id, cursor, 0)
}
pub(crate) use item_ingress::{ItemIngressCommand, ItemIngressMutation, apply_item_ingress};
pub(crate) use mutations::{StoreMutation, StoreMutationResult};
pub(crate) use paging::{RecoveryCandidate, load_recovery_candidates};
pub(crate) use request_terminal::apply_request_state;
#[cfg(test)]
pub(crate) use requests::DenyOnlyPermissionJournal;
pub(crate) use requests::receive_permission_request;
pub(crate) use residency::{ParkSessionCommand, park_session_after_zero_process};
pub(crate) use send::parse_turn_state;
#[cfg(test)]
pub(crate) use send::{begin_send, record_send_observation};
pub(crate) use session_close::{
    FinalizeCloseCommand, close_session, finalize_close_after_zero_process,
};
pub(crate) use session_head::{ended_head, preceding_session, runs_next_send};
pub(crate) use session_restart::{RestartSessionCommand, restart_undone_session};
pub(crate) use session_resume::{
    ResumeEarlierSessionCommand, link_resumed_session, unstarted_continuation,
};
pub(crate) use session_surface::SessionEventCommand;
pub(crate) use sessions::{
    ActivateSessionCommand, CloseSessionCommand, SessionMutation, StartSessionCommand,
    activate_session, start_session,
};
pub(crate) use snapshots::{FilePairCommand, read_snapshot};
#[cfg(test)]
pub(crate) use terminal::apply_session_terminal;
pub(crate) use terminal::apply_turn_terminal;
pub(crate) use terminalize::{terminalize_epoch, terminalize_recovery_epoch};
pub(crate) use turn_undo::{
    TurnUndoneCommand, UndoneThread, pending_thread_rewind, record_turn_undone,
};

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
pub(crate) struct RecoverRuntimeCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
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
    #[allow(dead_code, reason = "only tests construct this variant so far")]
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

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionTerminalCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub terminal: SessionState,
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
            > sage_core::limits::LIMITS
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
                terminal_reason,
            } => {
                let turn = send::record_send_observation_in(
                    &transaction,
                    command.clone(),
                    *observation,
                    terminal_reason.clone(),
                )?;
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
mod terminal_reason_tests;
#[cfg(test)]
mod tests;
