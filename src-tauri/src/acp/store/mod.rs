#![allow(dead_code)] // Removed when the isolated packet receives central wiring.

mod batch;
use batch::commit_batch;

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
mod history_roots;
mod item_ingress;
mod item_usage;
mod mutations;
mod paging;
mod permission_item;
mod projection_decode;
pub(crate) use projection_decode::parse_legacy_text;
pub(crate) mod context_handoff;
mod context_handoff_budget;
#[cfg(test)]
pub(crate) mod handoff_fixture;
mod request_identity;
mod request_terminal;
mod requests;
mod residency;
mod send;
mod session_close;
mod session_duplicates;
mod session_insert;
pub(crate) mod session_replace;
#[cfg(test)]
pub(crate) mod session_replace_tests;
pub(crate) mod sessions;
mod snapshots;
mod state;
mod terminal;
mod terminal_reason;
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
    AbortActivationCommand, ActivationAbortCategory, CompleteActivationCommand,
    FailActivationCommand, abort_activation, complete_activation, fail_activation,
};
#[allow(unused_imports)]
pub(crate) use activity_ingress::{
    ActivityIngressCommand, ActivityIngressMutation, apply_activity_ingress,
};
#[allow(unused_imports)]
pub(crate) use adapter_batch::{
    AdapterActivityItemCommand, AdapterBatchMutation, AdapterInboundBatchCommand,
    AdapterInboundEvent, AdapterStoreOwner, AdapterTurnCursor, ChildLiveItemCommand,
    TurnTerminalReasonCommand, TurnUsageSnapshotCommand, apply_adapter_inbound_batch,
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
pub(crate) use requests::{
    DenyOnlyPermissionJournal, PermissionDecisionJournal, receive_permission_request,
};
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
pub(crate) use snapshots::{FilePairCommand, read_snapshot};
#[allow(unused_imports)]
pub(crate) use terminal::{
    apply_activity_terminal, apply_item_terminal, apply_session_terminal, apply_turn_terminal,
};
pub(crate) use terminalize::terminalize_recovery_epoch;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
    #[error(
        "HandoffBudgetExceeded: historical coverage and the complete prompt cannot fit; draft retained"
    )]
    HandoffBudgetExceeded,
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

#[cfg(test)]
mod adapter_batch_tests;
#[cfg(test)]
mod attention_tests;
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
