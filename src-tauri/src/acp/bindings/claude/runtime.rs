use std::{collections::BTreeMap, sync::Arc};

use serde_json::{Value, json};

use crate::acp::{
    bindings::{
        adapter::{
            AdapterBatch, AdapterBatchBuilder, AdapterCommand, AdapterEgress,
            AdapterEvent as SharedEvent, AdapterModel, AdapterRuntimeError, EgressResolution,
            EgressToken, HarnessAdapter, RefusalDetail, ResourceMapper as SharedResourceMapper,
        },
        runtime::{LaunchableRuntime, NativeRuntime, RuntimeFactory},
        standard_acp::{
            AcpLaunchAuthority, ClaudeLaunch, profile_for, surface::auth_refusal_detail,
        },
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    lifecycle::TurnState,
    limits::LIMITS,
    mcp::{McpAdmission, McpCapabilities},
    process::LaunchSpec,
    runtime::{later_permission::resolve_later_permission, permissions::PermissionDecision},
    transport::{Frame, OutboundFrame, request_payload_digest},
};

use super::{
    AdapterEvent, CLAUDE_PROFILE_ID, CandidateManifest, ClaudeAuthState, ClaudeCapabilityOffer,
    ClaudeTranslator, PromptTerminal, admission::AdmittedClaudeBinding, decode_auth_required_error,
    decode_auth_status_update, negotiate_capabilities, validate_research_candidate,
    wire::decode_frame,
};

mod browse_host;
mod commands;
mod egress;
mod ingress;
mod session_config;
mod support;
use ingress::terminal_state;
use support::*;

const MAX_PENDING_REQUESTS: usize = 16;

pub(crate) struct ClaudeRuntime {
    launch_binding: Option<Arc<AdmittedClaudeBinding>>,
    pending_project_root: Option<crate::project_registry::ProjectRootAuthority>,
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    next_request_id: u64,
    capabilities: Option<CapabilitySnapshotV1>,
    mcp: McpAdmission,
    mcp_capabilities: McpCapabilities,
    default_model: Option<crate::acp::intent::ModelId>,
    pending: BTreeMap<u64, PendingRequest>,
    active: Option<ActiveSession>,
    auth: ClaudeAuthState,
    denials: BTreeMap<String, DenialRecord>,
    next_denial_sequence: u64,
    delivery_unknown: Option<DeliveryUnknown>,
    refusal_detail: Option<RefusalDetail>,
    deferred: Vec<AdapterEvent>,
}

struct ActiveSession {
    external_id: ExternalId,
    translator: ClaudeTranslator,
    turn: Option<TurnIntent>,
    config: crate::acp::session_config::SessionConfig,
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
    SetMode,
    SetConfig,
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
    allow_once_option_id: Option<String>,
    allow_always_option_id: Option<String>,
    reply: PermissionReply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionReply {
    AllowOnce,
    AllowAlways,
    Deny,
    Cancelled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeliveryUnknown {
    Request(u64),
    Notification,
}

impl ClaudeRuntime {
    pub(crate) fn from_candidate(
        candidate: CandidateManifest,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        let candidate = validate_research_candidate(candidate)
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        candidate
            .require_launch_admission()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self::new(None, binding_digest, runtime_epoch, session_id))
    }

    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedClaudeBinding>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<Self, AdapterRuntimeError> {
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self::new(
            Some(binding.clone()),
            binding.binding_digest(),
            runtime_epoch,
            session_id,
        ))
    }

    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        Self::new(
            None,
            [7; 32],
            RuntimeEpoch::new(3).expect("runtime epoch"),
            SageAgentSessionId::parse("session-claude-runtime").expect("session"),
        )
    }

    fn new(
        launch_binding: Option<Arc<AdmittedClaudeBinding>>,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            launch_binding,
            pending_project_root: None,
            binding_digest,
            runtime_epoch,
            session_id,
            next_request_id: 0,
            capabilities: None,
            mcp: McpAdmission::default(),
            mcp_capabilities: McpCapabilities::default(),
            default_model: None,
            pending: BTreeMap::new(),
            active: None,
            auth: ClaudeAuthState::default(),
            denials: BTreeMap::new(),
            next_denial_sequence: 0,
            delivery_unknown: None,
            refusal_detail: None,
            deferred: Vec::new(),
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

    fn launch_spec(
        &mut self,
        project_root: crate::project_registry::ProjectRootAuthority,
    ) -> Result<LaunchSpec, AdapterRuntimeError> {
        let binding = self
            .launch_binding
            .as_deref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let candidate = binding.candidate();
        let environment = crate::acp::process::BoundedEnvironment::new(
            binding
                .launch_environment()
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
        )
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        AcpLaunchAuthority::for_claude(ClaudeLaunch {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch.get(),
            runtime_path: candidate.runtime_path.clone(),
            runtime_sha256: candidate.runtime_sha256.clone(),
            runtime_byte_len: std::fs::metadata(&candidate.runtime_path)
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?
                .len(),
            entrypoint: candidate.artifact_path.clone(),
            environment,
        })?
        .into_launch_spec(project_root)
    }
}

pub(crate) struct ClaudeRuntimeFactory {
    binding: Arc<AdmittedClaudeBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    mcp: McpAdmission,
}

impl ClaudeRuntimeFactory {
    pub(crate) fn from_admitted_binding(
        binding: Arc<AdmittedClaudeBinding>,
        project_root: crate::project_registry::ProjectRootAuthority,
    ) -> Self {
        Self::with_mcp(binding, project_root, McpAdmission::default())
    }

    pub(crate) fn with_mcp(
        binding: Arc<AdmittedClaudeBinding>,
        project_root: crate::project_registry::ProjectRootAuthority,
        mcp: McpAdmission,
    ) -> Self {
        Self {
            binding,
            project_root,
            mcp,
        }
    }
}

impl RuntimeFactory for ClaudeRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        let Self {
            binding,
            project_root,
            mcp,
        } = *self;
        let mut runtime = ClaudeRuntime::from_admitted_binding(binding, runtime_epoch, session_id)?;
        runtime.mcp = mcp;
        runtime.pending_project_root = Some(project_root);
        Ok(NativeRuntime::new(runtime))
    }
}

impl LaunchableRuntime for ClaudeRuntime {
    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        let project_root = self
            .pending_project_root
            .take()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        ClaudeRuntime::launch_spec(self, project_root)
    }
}

impl HarnessAdapter for ClaudeRuntime {
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.active.as_ref().map(|a| a.config.clone())
    }
    fn refusal_detail(&self) -> Option<RefusalDetail> {
        self.refusal_detail.clone()
    }
    fn config_pending(&self) -> bool {
        self.pending
            .values()
            .any(|v| matches!(v, PendingRequest::SetMode | PendingRequest::SetConfig))
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.configure_session(intent)
    }
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.capabilities.clone()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.command_inner(command)
    }

    fn cancel_permission(
        &mut self,
        request_id: &crate::acp::contract::SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let record = self
            .denials
            .get(request_id.as_str())
            .filter(|record| record.state == DenialState::Pending)
            .ok_or(AdapterRuntimeError::PermissionIntent)?;
        cancelled_permission(request_id, &record.wire_id)
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
mod browser_mcp_tests;
#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod durable_turn_tests;
#[cfg(test)]
mod tests;
