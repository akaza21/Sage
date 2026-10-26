mod commands;
mod ingress;
mod launch;
use commands::response_id;
pub(super) use commands::scoped_permission_id;

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError},
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{CapabilitySnapshotV1, ExternalId},
    mcp::McpAdmission,
    transport::OutboundFrame,
};
use crate::project_registry::ProjectRootAuthority;

use super::{
    OpenCodeBindingRegistration, StandardAcpHandshake, profile_for,
    translator::{StandardAcpTranslator, TranslationError},
};

const MAX_PENDING_REQUESTS: usize = 3;

pub(super) struct OpenCodeRuntime {
    registration: Arc<OpenCodeBindingRegistration>,
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
    deferred: Vec<super::translator::AdapterEvent>,
    safety_alerted: bool,
}

struct ActiveSession {
    external_id: ExternalId,
    translator: StandardAcpTranslator,
    turn: Option<TurnIntent>,
}

struct TurnIntent {
    external_id: ExternalId,
    client_message_id: RequiredText<512>,
    request_id: u64,
    access_mode: crate::settings_store::SessionAccessMode,
}

struct DeferredPrompt {
    prompt: crate::acp::intent::Prompt,
    client_message_id: RequiredText<512>,
    mode_id: String,
    request_id: u64,
    cancelled: bool,
    access_mode: crate::settings_store::SessionAccessMode,
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
    Prompt,
    SetMode,
    CloseSession,
}

impl OpenCodeRuntime {
    pub(super) fn new(
        registration: Arc<OpenCodeBindingRegistration>,
        binding_digest: [u8; 32],
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
        project_root: ProjectRootAuthority,
        mcp: McpAdmission,
    ) -> Result<Self, AdapterRuntimeError> {
        let profile = profile_for("opencode").ok_or(AdapterRuntimeError::LaunchUnavailable)?;
        Ok(Self {
            registration,
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
            deferred: Vec::new(),
            safety_alerted: false,
        })
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
        self.reserve(PendingRequest::NewSession, value)
    }

    fn prompt(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.prompt)
            || self.delivery_unknown.is_some()
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if self.close_pending {
            return Err(AdapterRuntimeError::Operation);
        }
        let id = self.next_request_id.saturating_add(1);
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        active
            .translator
            .begin_prompt(&id.to_string())
            .map_err(map_translation)?;
        active.turn = Some(TurnIntent {
            external_id: ExternalId::new("externalTurnId", format!("opencode-prompt-{id}"))
                .map_err(|_| AdapterRuntimeError::Operation)?,
            client_message_id,
            request_id: id,
            access_mode,
        });
        let session_id = active.external_id.clone();
        match self.reserve(
            PendingRequest::Prompt,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id.as_str(),
                    "prompt": [{"type": "text", "text": prompt.as_str()}]
                }
            }),
        ) {
            Ok(egress) => Ok(egress),
            Err(error) => {
                let active = self.active.as_mut().ok_or(AdapterRuntimeError::Operation)?;
                active
                    .translator
                    .reject_prompt(&id.to_string())
                    .map_err(map_translation)?;
                active.turn = None;
                Err(error)
            }
        }
    }

    fn prompt_with_mode(
        &mut self,
        prompt: crate::acp::intent::Prompt,
        client_message_id: RequiredText<512>,
        access_mode: crate::settings_store::SessionAccessMode,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let mode_id = match access_mode {
            crate::settings_store::SessionAccessMode::ReadOnly
            | crate::settings_store::SessionAccessMode::WorkspaceWrite
            | crate::settings_store::SessionAccessMode::Ask => "build",
        };
        if !self
            .available_modes
            .iter()
            .any(|available| available == mode_id)
            || self.pending_prompt.is_some()
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if self.current_mode_id.as_deref() == Some(mode_id) {
            return self.prompt(prompt, client_message_id, access_mode);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() || self.close_pending {
            return Err(AdapterRuntimeError::Operation);
        }
        let mode_id = mode_id.to_owned();
        let id = self.next_request_id.saturating_add(1);
        let egress = self.reserve(
            PendingRequest::SetMode,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/set_mode",
                "params": {"sessionId": active.external_id.as_str(), "modeId": mode_id}
            }),
        )?;
        self.pending_prompt = Some(DeferredPrompt {
            prompt,
            client_message_id,
            mode_id,
            request_id: id,
            cancelled: false,
            access_mode,
        });
        Ok(egress)
    }

    fn cancel(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.cancel)
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_none() && self.pending_prompt.is_none() {
            return Err(AdapterRuntimeError::Operation);
        }
        if let Some(deferred) = self.pending_prompt.as_mut() {
            deferred.cancelled = true;
        }
        let frame = outbound(json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": active.external_id.as_str()}
        }))?;
        Ok(AdapterEgress::new(
            frame,
            crate::acp::bindings::adapter::EgressToken::cancel_notification(),
        ))
    }

    fn close_session(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.close_session)
        {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        if self.close_pending {
            return Err(AdapterRuntimeError::Operation);
        }
        let session_id = self
            .active
            .as_ref()
            .ok_or(AdapterRuntimeError::Operation)?
            .external_id
            .clone();
        let id = self.next_request_id.saturating_add(1);
        let egress = self.reserve(
            PendingRequest::CloseSession,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/close",
                "params": {"sessionId": session_id.as_str()}
            }),
        )?;
        if let Some(deferred) = self.pending_prompt.as_mut() {
            deferred.cancelled = true;
        }
        self.close_pending = true;
        Ok(egress)
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
