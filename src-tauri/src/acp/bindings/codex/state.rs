pub(crate) mod capabilities;
pub(crate) mod command_output_stream;
mod command_snapshots;
mod errors;
mod permissions;
mod turn_starts;
mod turns;

use errors::observed_platform;
pub(crate) use errors::{AdapterError, TurnStartedRejection};

use command_snapshots::CommandItemSnapshots;
pub(crate) use command_snapshots::MaterializedCommandItem;
use turn_starts::{CorrelationMatch, TurnStartCorrelations};

use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId, ModelId, Prompt},
    lifecycle::{ItemLifecycle, ServerRequestState, TurnState},
    limits::LIMITS,
};

use super::{
    admission::AdmittedCodexBinding,
    incoming::{IngressDisposition, ResourceMapper, ingest},
    manifest::{CuratedCapabilityCeiling, MetadataValidatedPacket, Platform},
    request::{NativeProjectRoot, OutgoingRequest, RequestKind, initialized_notification},
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

#[derive(Debug)]
struct TurnTombstone {
    id: ExternalId,
    client_message_id: RequiredText<512>,
    state: TurnState,
    item_terminals: Vec<ItemTerminal>,
    terminal_id_bytes: usize,
}

#[derive(Debug, Clone)]
struct NegotiatedMethods {
    snapshot: CapabilitySnapshotV1,
    models: bool,
}

#[derive(Debug)]
struct PermissionRequestEntry {
    id: String,
    fingerprint: u64,
    method: String,
    turn_id: String,
    item_id: String,
    response: Vec<u8>,
    state: ServerRequestState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnBinding {
    NewlyActive,
    Active,
    PreviousTerminal,
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

pub(crate) struct CodexAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    platform: Platform,
    codex_home: String,
    runtime_version: String,
    capability_ceiling: CuratedCapabilityCeiling,
    pending: Vec<(u64, RequestKind)>,
    negotiated: Option<NegotiatedMethods>,
    external_thread_id: Option<ExternalId>,
    turn_start_correlations: TurnStartCorrelations,
    active_turn_id: Option<ExternalId>,
    active_turn_client_message_id: Option<RequiredText<512>>,
    active_item_terminals: Vec<ItemTerminal>,
    active_terminal_id_bytes: usize,
    previous_turn: Option<TurnTombstone>,
    command_item_snapshots: CommandItemSnapshots,
    permission_requests: Vec<PermissionRequestEntry>,
}

impl CodexAdapter {
    pub(crate) fn from_metadata_packet(
        packet: &MetadataValidatedPacket,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            binding_digest,
            runtime_epoch,
            session_id,
            platform: packet.metadata().platform,
            codex_home: packet.codex_home().to_string_lossy().into_owned(),
            runtime_version: packet.metadata().codex_runtime_version.clone(),
            capability_ceiling: packet.capability_ceiling().clone(),
            pending: Vec::new(),
            negotiated: None,
            external_thread_id: None,
            turn_start_correlations: TurnStartCorrelations::new(),
            active_turn_id: None,
            active_turn_client_message_id: None,
            active_item_terminals: Vec::new(),
            active_terminal_id_bytes: 0,
            previous_turn: None,
            command_item_snapshots: CommandItemSnapshots::default(),
            permission_requests: Vec::new(),
        }
    }

    pub(crate) fn from_admitted_binding(
        binding: &AdmittedCodexBinding,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        let mut adapter = Self::from_metadata_packet(
            binding.packet(),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        );
        if binding.is_observed() {
            adapter.runtime_version = binding.harness_version().to_owned();
            adapter.codex_home.clear();
            adapter.platform = observed_platform();
        }
        adapter
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

    pub(crate) fn project_root(&self, value: String) -> Result<NativeProjectRoot, AdapterError> {
        NativeProjectRoot::new(self.platform, value).map_err(AdapterError::Request)
    }

    pub(crate) fn initialize(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        if self.negotiated.is_some()
            || self
                .pending
                .iter()
                .any(|(_, kind)| *kind == RequestKind::Initialize)
        {
            return Err(AdapterError::AlreadyInitialized);
        }
        self.reserve(OutgoingRequest::initialize(id)?)
    }

    pub(crate) fn initialized(&self) -> Result<Vec<u8>, AdapterError> {
        self.require_method(|_| true)?;
        initialized_notification().map_err(AdapterError::Request)
    }

    pub(crate) fn start(
        &mut self,
        id: u64,
        root: &NativeProjectRoot,
        model: Option<&ModelId>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.new_session)?;
        self.reserve(OutgoingRequest::start(id, root, model)?)
    }

    pub(crate) fn resume(
        &mut self,
        id: u64,
        external_thread_id: &ExternalId,
        root: &NativeProjectRoot,
        model: Option<&ModelId>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.resume_session)?;
        self.reserve(OutgoingRequest::resume(
            id,
            external_thread_id,
            root,
            model,
        )?)
    }

    pub(crate) fn read(
        &mut self,
        id: u64,
        external_thread_id: &ExternalId,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.load_session)?;
        self.reserve(OutgoingRequest::read(id, external_thread_id)?)
    }

    pub(crate) fn models(
        &mut self,
        id: u64,
        cursor: Option<&RequiredText<512>>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.models)?;
        self.reserve(OutgoingRequest::models(id, cursor)?)
    }

    pub(crate) fn prompt(
        &mut self,
        id: u64,
        prompt: &Prompt,
        client_message_id: &RequiredText<512>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.prompt)?;
        if self.turn_start_correlations.has_pending() || self.active_turn_id.is_some() {
            return Err(AdapterError::TurnAlreadyActive);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = self.reserve(OutgoingRequest::turn_start(
            id,
            &thread,
            prompt,
            client_message_id,
        )?)?;
        self.turn_start_correlations
            .register(id, client_message_id.clone());
        Ok(request)
    }

    pub(crate) fn cancel(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.cancel)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let turn = self
            .active_turn_id
            .clone()
            .ok_or(AdapterError::TurnUnavailable)?;
        self.reserve(OutgoingRequest::interrupt(id, &thread, &turn)?)
    }

    pub(crate) fn close(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.close_session)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        self.reserve(OutgoingRequest::unsubscribe(id, &thread)?)
    }

    pub(crate) fn reject_before_write(&mut self, id: u64) -> Result<(), AdapterError> {
        let kind = self.pending_kind(id).ok_or(AdapterError::ForeignRequest)?;
        if kind == RequestKind::TurnStart {
            self.turn_start_correlations.discard_pending(id)?;
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
        if self.pending_kind(id) != Some(RequestKind::TurnStart) {
            return Err(AdapterError::TurnIntentConflict);
        }
        let correlation = self.turn_start_correlations.failure(id)?;
        let result = match correlation {
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
        };
        self.finish_turn_start_response(id)?;
        Ok(result)
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
        let scoped = self.scoped_in_turn(ExternalKind::Turn, &id, &id)?;
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
        if self.pending_kind(id) != Some(RequestKind::TurnStart) {
            return Err(AdapterError::TurnIntentConflict);
        }
        self.turn_start_correlations.consume(id)?;
        let removed = self.remove_pending(id);
        debug_assert_eq!(removed, Some(RequestKind::TurnStart));
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
