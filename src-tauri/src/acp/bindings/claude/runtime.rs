use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterBatchBuilder, AdapterCommand, AdapterEgress,
        AdapterEvent as SharedEvent, AdapterModel, AdapterRuntimeError, EgressResolution,
        EgressToken, HarnessAdapter, ResourceMapper as SharedResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    lifecycle::TurnState,
    limits::LIMITS,
    runtime::{later_permission::resolve_later_permission, permissions::PermissionDecision},
    transport::{Frame, OutboundFrame, request_payload_digest},
};

use super::{
    AdapterEvent, CandidateManifest, ClaudeCapabilityOffer, ClaudeTranslator, PromptTerminal,
    negotiate_capabilities, validate_research_candidate, wire::decode_frame,
};

mod browse_host;
mod commands;
mod egress;
mod ingress;
#[path = "runtime/support.rs"]
mod support;
use ingress::terminal_state;
use support::*;

const MAX_PENDING_REQUESTS: usize = 16;

pub(crate) struct ClaudeRuntime {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    next_request_id: u64,
    capabilities: Option<CapabilitySnapshotV1>,
    default_model: Option<crate::acp::intent::ModelId>,
    pending: BTreeMap<u64, PendingRequest>,
    active: Option<ActiveSession>,
    denials: BTreeMap<String, DenialRecord>,
    next_denial_sequence: u64,
    delivery_unknown: Option<DeliveryUnknown>,
}

struct ActiveSession {
    external_id: ExternalId,
    translator: ClaudeTranslator,
    turn: Option<TurnIntent>,
}

#[derive(Clone)]
struct TurnIntent {
    external_turn_id: ExternalId,
    client_message_id: RequiredText<512>,
    request_id: u64,
}

enum PendingRequest {
    Initialize,
    NewSession,
    ResumeSession(ExternalId),
    CloseSession,
    Prompt,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DenialState {
    Pending,
    Committed,
    Queued,
    Written,
}

struct DenialRecord {
    state: DenialState,
    sequence: u64,
    translator_id: String,
    wire_id: Value,
    allow_once_option_id: String,
    reply: PermissionReply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionReply {
    AllowOnce,
    Deny,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeliveryUnknown {
    Request(u64),
    Notification,
}

impl ClaudeRuntime {
    pub(crate) fn from_candidate(
        candidate: CandidateManifest,
        forbidden_state_dirs: &[std::path::PathBuf],
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        let candidate = validate_research_candidate(candidate, forbidden_state_dirs)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        candidate
            .require_launch_admission()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self::new(binding_digest, runtime_epoch, session_id))
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Self::new(
            [7; 32],
            RuntimeEpoch::new(3).expect("runtime epoch"),
            SageAgentSessionId::parse("session-claude-runtime").expect("session"),
        )
    }

    fn new(
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            binding_digest,
            runtime_epoch,
            session_id,
            next_request_id: 0,
            capabilities: None,
            default_model: None,
            pending: BTreeMap::new(),
            active: None,
            denials: BTreeMap::new(),
            next_denial_sequence: 0,
            delivery_unknown: None,
        }
    }

    fn reserve(
        &mut self,
        pending: PendingRequest,
        method: &'static str,
        params: Value,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.delivery_unknown.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        if self.pending.len() >= MAX_PENDING_REQUESTS {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let id = self
            .next_request_id
            .checked_add(1)
            .filter(|id| *id <= i64::MAX as u64)
            .ok_or(AdapterRuntimeError::Operation)?;
        let egress = request(id, method, params)?;
        self.pending.insert(id, pending);
        self.next_request_id = id;
        Ok(egress)
    }

    fn require_capability(
        &self,
        select: impl FnOnce(&CapabilitySnapshotV1) -> bool,
    ) -> Result<(), AdapterRuntimeError> {
        self.capabilities
            .as_ref()
            .filter(|capabilities| select(capabilities))
            .map(|_| ())
            .ok_or(AdapterRuntimeError::UnsupportedCommand)
    }

    fn accepts_default_model(&self, model: Option<&crate::acp::intent::ModelId>) -> bool {
        model.is_none_or(|model| {
            self.default_model
                .as_ref()
                .is_some_and(|default| default == model)
        })
    }
}

impl HarnessAdapter for ClaudeRuntime {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.capabilities.clone()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.command_inner(command)
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn SharedResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.ingest_inner(frame, resources)
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.commit_mandatory_egress_inner(token)
    }

    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.resolve_egress_inner(token, resolution)
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
