mod action_handoff;
pub(crate) mod capabilities;
mod child_read;
mod fork;
#[cfg(test)]
pub(crate) use fork::MAX_FORK_RELEASE_ATTEMPTS;
pub(crate) use fork::{MAX_UNRELEASED_FORKS, fork_reply};
pub(crate) mod command_explore;
pub(crate) mod command_output_stream;
mod command_snapshots;
mod construction;
mod errors;
mod goal;
mod permission_resolution;
mod permissions;
pub(in crate::acp::bindings::codex) use permissions::USER_INPUT_METHOD;
mod provisional;
mod rate_limits;
mod session;
mod turn_errors;
mod turn_prompt;
mod turn_starts;
mod turns;

use crate::acp::goal::GoalState;
pub(crate) use errors::{AdapterError, TurnStartedRejection};

use command_snapshots::CommandItemSnapshots;
pub(crate) use command_snapshots::MaterializedCommandItem;
use turn_starts::{CorrelationMatch, TurnStartCorrelations};
pub(in crate::acp::bindings::codex) use turns::SubagentLabels;

use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId, session_event::SessionScopedEvent},
    lifecycle::{ItemLifecycle, ServerRequestState, TurnState},
    limits::LIMITS,
};

use super::{
    incoming::{IngressDisposition, ResourceMapper, ingest},
    manifest::{CuratedCapabilityCeiling, Platform},
    request::{OutgoingRequest, RequestKind},
};

const TERMINAL_ID_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalKind {
    Session,
    Turn,
    Item,
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopedExternalId {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) turn_id: Option<ExternalId>,
    pub(crate) turn_client_message_id: Option<RequiredText<512>>,
    pub(crate) kind: ExternalKind,
    pub(crate) external_id: ExternalId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemTerminal {
    id: String,
    state: ItemLifecycle,
}

#[derive(Clone, Debug, PartialEq)]
struct TurnTombstone {
    id: ExternalId,
    client_message_id: RequiredText<512>,
    state: TurnState,
    item_terminals: Vec<ItemTerminal>,
    terminal_id_bytes: usize,
    unretried_error_fingerprints: Vec<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq)]
struct NegotiatedMethods {
    snapshot: CapabilitySnapshotV1,
    models: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct PermissionRequestEntry {
    id: String,
    fingerprint: u64,
    method: String,
    allow_always: bool,
    turn_id: String,
    item_id: String,
    response: Vec<u8>,
    state: ServerRequestState,
}

pub(crate) struct PermissionRequestParams<'a> {
    pub(crate) request_id: &'a str,
    pub(crate) fingerprint: u64,
    pub(crate) method: &'a str,
    pub(crate) allow_always: bool,
    pub(crate) turn_id: &'a str,
    pub(crate) item_id: &'a str,
    pub(crate) response: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnBinding {
    NewlyActive,
    Active,
    PreviousTerminal,
    Provisional,
}

#[derive(Clone, Debug, PartialEq)]
struct ProvisionalTurn {
    response_id: ExternalId,
    started_id: Option<ExternalId>,
    client_message_id: RequiredText<512>,
    deadline: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TurnStartFailure {
    Failed(ScopedExternalId),
    Late,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionRegistration {
    New(ScopedExternalId),
    DuplicatePending,
    DuplicateCommitted(Vec<u8>),
}

/// Cloned and compared only to probe how ingestion would read frames without
/// applying them.
#[derive(Clone, PartialEq)]
pub(crate) struct CodexAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    platform: Platform,
    codex_home: String,
    runtime_version: String,
    capability_ceiling: CuratedCapabilityCeiling,
    observed_build_characterisation:
        Option<crate::acp::bindings::characterisation::Characterisation>,
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    desktop_fixture: bool,
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    desktop_fixture_lists_models: bool,
    pending: Vec<(u64, RequestKind)>,
    negotiated: Option<NegotiatedMethods>,
    external_thread_id: Option<ExternalId>,
    browser_mcp_failure_reported: bool,
    pending_browser_mcp_failure_threads: Vec<String>,
    /// Thread-less notices that arrived before a thread was bound; at most
    /// `PENDING_NOTICES_MAX`, oldest dropped first.
    pending_session_notices: Vec<SessionScopedEvent>,
    turn_start_correlations: TurnStartCorrelations,
    active_turn_id: Option<ExternalId>,
    provisional_turn: Option<ProvisionalTurn>,
    action_handoff: Option<action_handoff::ActionHandoff>,
    deferred_interrupt: bool,
    active_turn_client_message_id: Option<RequiredText<512>>,
    active_item_terminals: Vec<ItemTerminal>,
    active_terminal_id_bytes: usize,
    active_unretried_error_fingerprints: Vec<[u8; 32]>,
    previous_turn: Option<TurnTombstone>,
    command_item_snapshots: CommandItemSnapshots,
    subagent_labels: SubagentLabels,
    child_threads: Vec<SafeText<512>>,
    bound_child_threads: Vec<SafeText<512>>,
    unknown_child_notification_reported: bool,
    permission_requests: Vec<PermissionRequestEntry>,
    goal_state: GoalState,
    goal_notification_generation: u64,
    goal_get_fence: Option<(u64, u64)>,
    goal_mutation_fence: Option<(u64, u64)>,
    pending_child_read: Option<(u64, String, String)>,
    child_read_tombstone: Option<u64>,
    rate_limits_abandoned: rate_limits::AbandonedReads,
    pending_fork: Option<u64>,
    forks: fork::ForkLedger,
}

impl CodexAdapter {
    pub(crate) fn has_active_turn(&self) -> bool {
        self.active_turn_id.is_some()
    }

    /// A nested turn's root names the active Turn, or, before any Turn is
    /// named, the review whose `review/start` answer has not arrived yet.
    pub(crate) fn root_names_turn(&self, root: &str) -> bool {
        match &self.active_turn_id {
            Some(active) => active.as_str() == root,
            None => {
                let mut starts = self.pending.iter().filter(|(_, kind)| kind.starts_turn());
                matches!(
                    (starts.next(), starts.next()),
                    (Some((_, RequestKind::Review)), None)
                )
            }
        }
    }

    pub(crate) fn mark_browser_mcp_failure_reported(&mut self) -> bool {
        if self.browser_mcp_failure_reported {
            return false;
        }
        self.browser_mcp_failure_reported = true;
        true
    }

    pub(crate) fn has_bound_thread(&self) -> bool {
        self.external_thread_id.is_some()
    }

    pub(crate) fn effective_capabilities(&self) -> Result<CapabilitySnapshotV1, AdapterError> {
        self.negotiated
            .as_ref()
            .map(|value| value.snapshot.clone())
            .ok_or(AdapterError::CapabilityUnavailable)
    }

    pub(crate) fn runtime_epoch(&self) -> RuntimeEpoch {
        self.runtime_epoch
    }

    pub(crate) fn session_id(&self) -> &SageAgentSessionId {
        &self.session_id
    }

    pub(crate) fn reject_before_write(&mut self, id: u64) -> Result<(), AdapterError> {
        let kind = self.pending_kind(id).ok_or(AdapterError::ForeignRequest)?;
        if kind.starts_turn() {
            self.turn_start_correlations.discard_pending(id)?;
            self.deferred_interrupt = false;
        }
        if kind == RequestKind::GoalGet {
            self.goal_get_failed(id);
        }
        if matches!(kind, RequestKind::GoalPause | RequestKind::GoalClear) {
            self.goal_mutation_fence = None;
        }
        if kind == RequestKind::ChildRead {
            self.pending_child_read = None;
        }
        if kind == RequestKind::Fork {
            self.pending_fork = None;
        }
        if matches!(kind, RequestKind::ForkRelease | RequestKind::ForkDelete) {
            self.forks.release_unwritten(id);
        }
        let removed = self.remove_pending(id);
        debug_assert_eq!(removed, Some(kind));
        Ok(())
    }

    pub(crate) fn pending_kind(&self, id: u64) -> Option<RequestKind> {
        self.pending
            .iter()
            .find_map(|(pending, kind)| (*pending == id).then_some(*kind))
    }

    pub(crate) fn ingest<M: ResourceMapper>(
        &mut self,
        bytes: &[u8],
        mapper: &M,
    ) -> Result<IngressDisposition, AdapterError> {
        ingest(self, bytes, mapper).map_err(AdapterError::Ingress)
    }

    pub(crate) fn ingest_with_declarations<M: ResourceMapper>(
        &mut self,
        bytes: &[u8],
        mapper: &M,
        declarations: &crate::acp::declared_test::DeclaredTestCommands,
    ) -> Result<IngressDisposition, AdapterError> {
        super::incoming::ingest_with_declarations(self, bytes, mapper, declarations)
            .map_err(AdapterError::Ingress)
    }

    pub(super) fn take_pending(&mut self, id: u64) -> Option<RequestKind> {
        self.remove_pending(id)
    }

    pub(super) fn fail_turn_start(&mut self, id: u64) -> Result<TurnStartFailure, AdapterError> {
        if !self.pending_kind(id).is_some_and(RequestKind::starts_turn) {
            return Err(AdapterError::TurnIntentConflict);
        }
        let result = self.fail_correlation(id)?;
        self.finish_turn_start_response(id)?;
        Ok(result)
    }

    fn fail_correlation(&mut self, id: u64) -> Result<TurnStartFailure, AdapterError> {
        Ok(match self.turn_start_correlations.failure(id)? {
            CorrelationMatch::Pending(client) => {
                self.active_turn_client_message_id = Some(client);
                self.active_turn_id =
                    Some(ExternalId::new("externalTurnId", format!("rejected-{id}"))?);
                self.accept_active_failure().map(TurnStartFailure::Failed)?
            }
            CorrelationMatch::Active { turn_id, .. }
                if self.active_turn_id.as_ref() == Some(&turn_id) =>
            {
                self.accept_active_failure().map(TurnStartFailure::Failed)?
            }
            CorrelationMatch::Terminal { .. } => TurnStartFailure::Late,
            CorrelationMatch::Active { .. } => return Err(AdapterError::TurnIntentConflict),
        })
    }

    pub(super) fn fail_pending_turn_start(&mut self) -> Result<TurnStartFailure, AdapterError> {
        let mut turn_starts = self
            .pending
            .iter()
            .filter_map(|(id, kind)| kind.starts_turn().then_some(*id));
        match (turn_starts.next(), turn_starts.next()) {
            (Some(id), None) => self.fail_turn_start(id),
            (None, None) if self.action_handoff.is_some() => self.fail_action_handoff(),
            _ => Err(AdapterError::TurnIntentConflict),
        }
    }

    pub(super) fn fail_interrupt(&mut self) -> Result<ScopedExternalId, AdapterError> {
        self.accept_active_failure()
    }

    fn accept_active_failure(&mut self) -> Result<ScopedExternalId, AdapterError> {
        let id = self
            .active_turn_id
            .as_ref()
            .ok_or(AdapterError::TurnUnavailable)?
            .as_str()
            .to_owned();
        let scoped = self.scoped_turn(&id)?;
        self.accept_turn_terminal(&id, TurnState::Failed)?;
        Ok(scoped)
    }

    fn remove_pending(&mut self, id: u64) -> Option<RequestKind> {
        let index = self
            .pending
            .iter()
            .position(|(pending, _)| *pending == id)?;
        Some(self.pending.swap_remove(index).1)
    }

    pub(super) fn finish_turn_start_response(&mut self, id: u64) -> Result<(), AdapterError> {
        if !self.pending_kind(id).is_some_and(RequestKind::starts_turn) {
            return Err(AdapterError::TurnIntentConflict);
        }
        self.turn_start_correlations.consume(id)?;
        self.remove_pending(id);
        Ok(())
    }

    fn reserve(&mut self, request: OutgoingRequest) -> Result<OutgoingRequest, AdapterError> {
        if self.pending.len() >= LIMITS.queues.outstanding_rpc_per_session as usize {
            return Err(AdapterError::PendingFull);
        }
        if self.pending.iter().any(|(id, _)| *id == request.id) {
            return Err(AdapterError::DuplicateRequestId);
        }
        self.pending.push((request.id, request.kind));
        Ok(request)
    }

    fn require_method(
        &self,
        available: impl FnOnce(&NegotiatedMethods) -> bool,
    ) -> Result<(), AdapterError> {
        match self.negotiated.as_ref() {
            Some(methods) if available(methods) => Ok(()),
            _ => Err(AdapterError::CapabilityUnavailable),
        }
    }

    fn scoped(&self, kind: ExternalKind, external_id: ExternalId) -> ScopedExternalId {
        ScopedExternalId {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            turn_id: None,
            turn_client_message_id: None,
            kind,
            external_id,
        }
    }
}
