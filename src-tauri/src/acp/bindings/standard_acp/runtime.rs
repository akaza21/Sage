mod auth;
mod commands;
mod elicitation;
mod factory;
mod ingress;
mod launch;
mod turn;
use commands::response_id;
pub(super) use commands::scoped_permission_id;
pub(crate) use factory::{StandardAcpBindingSource, StandardAcpRuntimeFactory};

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use serde_json::Value;

use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, RefusalDetail},
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    mcp::McpAdmission,
    transport::OutboundFrame,
};
use crate::project_registry::ProjectRootAuthority;

use super::{
    StandardAcpHandshake, StandardAcpProfile,
    translator::{StandardAcpTranslator, TranslationError},
};

const MAX_PENDING_REQUESTS: usize = 3;

pub(super) struct StandardAcpRuntime {
    profile: &'static StandardAcpProfile,
    source: Arc<dyn StandardAcpBindingSource>,
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: SageAgentSessionId,
    project_root: Option<ProjectRootAuthority>,
    workspace_root: PathBuf,
    workspace_identity: [u8; 24],
    handshake: StandardAcpHandshake,
    next_request_id: u64,
    pending: BTreeMap<u64, PendingRequest>,
    capabilities: Option<CapabilitySnapshotV1>,
    active: Option<ActiveSession>,
    close_pending: bool,
    delivery_unknown: Option<u64>,
    current_mode_id: Option<String>,
    available_modes: Vec<String>,
    pending_prompt: Option<DeferredPrompt>,
    permissions: BTreeMap<String, PermissionIntent>,
    elicitations: BTreeMap<String, elicitation::ElicitationIntent>,
    sign_in: auth::SignIn,
    refusal_detail: Option<RefusalDetail>,
    deferred: Vec<super::translator::AdapterEvent>,
    safety_alerted: bool,
}

struct ActiveSession {
    commands: std::sync::Arc<crate::acp::slash_commands::CommandState>,
    external_id: ExternalId,
    translator: StandardAcpTranslator,
    turn: Option<TurnIntent>,
}

struct TurnIntent {
    external_id: ExternalId,
    client_message_id: RequiredText<512>,
    request_id: u64,
    access_mode: crate::acp_protocol::access_mode::SessionAccessMode,
}

struct DeferredPrompt {
    prompt: crate::acp::intent::Prompt,
    client_message_id: RequiredText<512>,
    mode_id: String,
    request_id: u64,
    cancelled: bool,
    access_mode: crate::acp_protocol::access_mode::SessionAccessMode,
}

struct PermissionIntent {
    translator_id: String,
    tool_id: String,
    request_digest: [u8; 32],
    wire_id: Value,
    allow_once_option_id: Option<String>,
    allow_always_option_id: Option<String>,
    reply: PermissionReply,
    state: PermissionState,
    turn_request_id: u64,
    approved_path: Option<PathBuf>,
    client_write_used: bool,
    created_file_content: Option<String>,
    created_file_written: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionReply {
    Deny,
    AllowOnce,
    AllowAlways,
    AwaitUser,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionState {
    Pending,
    Committed,
    Queued,
    Written,
}

enum PendingRequest {
    Initialize,
    NewSession,
    Authenticate,
    Prompt,
    SetMode,
    CloseSession,
}

impl StandardAcpRuntime {
    pub(super) fn new(
        profile: &'static StandardAcpProfile,
        source: Arc<dyn StandardAcpBindingSource>,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
        project_root: ProjectRootAuthority,
        mcp: McpAdmission,
    ) -> Self {
        Self {
            profile,
            source,
            binding_digest,
            runtime_epoch,
            session_id,
            workspace_root: project_root.as_path().to_path_buf(),
            workspace_identity: project_root.identity_bytes(),
            project_root: Some(project_root),
            handshake: StandardAcpHandshake::with_mcp(profile, mcp),
            next_request_id: 0,
            pending: BTreeMap::new(),
            capabilities: None,
            active: None,
            close_pending: false,
            delivery_unknown: None,
            current_mode_id: None,
            available_modes: Vec::new(),
            pending_prompt: None,
            permissions: BTreeMap::new(),
            elicitations: BTreeMap::new(),
            sign_in: auth::SignIn::default(),
            refusal_detail: None,
            deferred: Vec::new(),
            safety_alerted: false,
        }
    }

    fn reserve(
        &mut self,
        pending: PendingRequest,
        value: Value,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.delivery_unknown.is_some() || self.pending.len() >= MAX_PENDING_REQUESTS {
            return Err(AdapterRuntimeError::Operation);
        }
        let id = response_id(&value)?;
        if id != self.next_request_id.saturating_add(1) {
            return Err(AdapterRuntimeError::Operation);
        }
        let frame = outbound(value)?;
        self.pending.insert(id, pending);
        self.next_request_id = id;
        Ok(AdapterEgress::new(
            frame,
            crate::acp::bindings::adapter::EgressToken::request(id),
        ))
    }

    fn initialize(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.capabilities.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let value = self
            .handshake
            .initialize()
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.reserve(PendingRequest::Initialize, value)
    }

    fn new_session(&mut self, native_root: String) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.new_session)
            || self.active.is_some()
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let root = RequiredText::<16_384>::new("nativeRoot", native_root)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let value = self
            .handshake
            .new_session(root.as_str())
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let egress = self.reserve(PendingRequest::NewSession, value)?;
        self.sign_in.remember_root(root.as_str());
        Ok(egress)
    }

    fn turn_external_id(&self, kind: &str, id: u64) -> Result<ExternalId, AdapterRuntimeError> {
        ExternalId::new(
            "externalTurnId",
            format!("{}-{kind}-{id}", self.profile.harness_id),
        )
        .map_err(|_| AdapterRuntimeError::Frame)
    }
}

fn permission_identity(
    value: Option<&Value>,
) -> Result<(String, String, Value), AdapterRuntimeError> {
    let (kind, id, wire) = match value {
        Some(Value::String(value)) if !value.is_empty() && value.len() <= 512 => {
            ("string", value.clone(), Value::String(value.clone()))
        }
        Some(Value::Number(value)) => {
            let id = value
                .as_u64()
                .filter(|id| *id <= i64::MAX as u64)
                .ok_or(AdapterRuntimeError::Frame)?;
            ("number", id.to_string(), Value::Number(value.clone()))
        }
        _ => return Err(AdapterRuntimeError::Frame),
    };
    let token = format!("{kind}:{id}");
    Ok((token, id, wire))
}

fn outbound(value: Value) -> Result<OutboundFrame, AdapterRuntimeError> {
    serde_json::to_vec(&value)
        .map_err(|_| AdapterRuntimeError::Frame)
        .and_then(|bytes| OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) fn launch_environment_for_test(
    profile: &StandardAcpProfile,
    extension: &super::admission::StandardAcpLaunchExtension,
    read: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    launch::launch_environment(profile, extension, read).expect("launch environment")
}

pub(super) fn map_translation(error: TranslationError) -> AdapterRuntimeError {
    match error {
        TranslationError::EventLimit
        | TranslationError::ToolLimit
        | TranslationError::ItemLimit
        | TranslationError::PermissionLimit => AdapterRuntimeError::BatchCount,
        TranslationError::PlanLimit => AdapterRuntimeError::BatchBytes,
        _ => AdapterRuntimeError::Frame,
    }
}
