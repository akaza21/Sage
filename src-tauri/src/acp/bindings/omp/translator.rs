mod permissions;
mod requests;
mod responses;
mod updates;

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;

use crate::acp::{
    contract::{BoundedVec, ContractError, RequiredText, SafeText},
    intent::{COMMAND_RESOURCE_LIMIT, ExternalId, SafeResourceLocation},
    mcp::{McpAdmission, McpCapabilities},
};

use super::{
    manifest::OmpBindingCapabilities,
    protocol::{AdapterEvent, PromptTerminal},
    wire::{WireError, decode_frame},
};

pub(super) const MAX_PENDING_REQUESTS: usize = 16;
pub(super) const MAX_COMPLETED_REQUESTS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PendingKind {
    Initialize,
    NewSession,
    LoadSession { expected_session: String },
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletedResponse {
    fingerprint: u64,
}

#[derive(Debug)]
pub(super) struct ActiveTurn {
    turn_key: String,
    request_id: String,
    cancel_requested: bool,
    cancel_dispatch_required: bool,
    running_emitted: bool,
    terminal: Option<PromptTerminal>,
    tools: BTreeMap<String, crate::acp::contract::ToolState>,
    tool_categories: BTreeMap<String, crate::acp::intent::ToolCategory>,
    settled_reports: BTreeMap<String, crate::acp::contract::ToolState>,
    tool_snapshots: BTreeMap<
        String,
        (
            Option<SafeText<65536>>,
            BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        ),
    >,
    tool_snapshot_bytes: usize,
}

pub(crate) trait ResourceMapper {
    /// The native absolute path is transient and must never survive mapping.
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>;
}

#[derive(Debug)]
pub(crate) struct OmpTranslator {
    epoch: u64,
    next_request: u64,
    capabilities: OmpBindingCapabilities,
    initialized: bool,
    negotiated_load: bool,
    mcp: McpAdmission,
    mcp_capabilities: McpCapabilities,
    pending: BTreeMap<String, PendingKind>,
    completed: BTreeMap<String, CompletedResponse>,
    completed_order: VecDeque<String>,
    session_id: Option<ExternalId>,
    active: Option<ActiveTurn>,
    pending_permissions: BTreeMap<String, permissions::PermissionIntent>,
    resolved_permissions: BTreeMap<String, permissions::ResolvedPermission>,
}

impl OmpTranslator {
    pub(crate) fn new(epoch: u64, capabilities: OmpBindingCapabilities) -> Self {
        Self::with_mcp(epoch, capabilities, McpAdmission::default())
    }

    pub(crate) fn with_mcp(
        epoch: u64,
        capabilities: OmpBindingCapabilities,
        mcp: McpAdmission,
    ) -> Self {
        Self {
            epoch,
            next_request: 0,
            capabilities,
            initialized: false,
            negotiated_load: false,
            mcp,
            mcp_capabilities: McpCapabilities::default(),
            pending: BTreeMap::new(),
            completed: BTreeMap::new(),
            completed_order: VecDeque::new(),
            session_id: None,
            active: None,
            pending_permissions: BTreeMap::new(),
            resolved_permissions: BTreeMap::new(),
        }
    }

    pub(crate) fn ingest(
        &mut self,
        raw: &[u8],
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let frame = decode_frame(raw)?;
        if frame.get("method").and_then(Value::as_str) == Some("session/update") {
            return self.translate_update(&frame, resources);
        }
        if frame.get("method").and_then(Value::as_str) == Some("session/request_permission") {
            return self.translate_permission_request(&frame);
        }
        if frame.get("method").is_some() {
            return Err(TranslationError::UnexpectedMethod);
        }
        self.translate_response(&frame)
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn reject_before_write(&mut self, request_id: &str) -> Result<(), TranslationError> {
        let kind = self
            .pending
            .remove(request_id)
            .ok_or(TranslationError::Correlation)?;
        if kind == PendingKind::Prompt {
            let active = self.active.as_ref().ok_or(TranslationError::Correlation)?;
            if active.request_id != request_id {
                return Err(TranslationError::Correlation);
            }
            self.active = None;
        }
        Ok(())
    }

    pub(crate) fn reject_cancel_before_write(&mut self) -> Result<(), TranslationError> {
        let active = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        if active.terminal.is_some() || !active.cancel_requested {
            return Err(TranslationError::StaleIntent);
        }
        active.cancel_requested = false;
        Ok(())
    }
}

pub(super) fn validated_id(
    field: &'static str,
    value: String,
) -> Result<ExternalId, TranslationError> {
    RequiredText::new(field, value).map_err(Into::into)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TranslationError {
    #[error("OMP ACP operation is unsupported by pinned evidence: {0}")]
    UnsupportedOperation(&'static str),
    #[error("OMP ACP lifecycle operation is not valid in the current state")]
    InvalidLifecycle,
    #[error("OMP ACP session transition is blocked by active or pending work")]
    SessionTransitionBusy,
    #[error("OMP ACP prompt already has an active Turn")]
    PromptAlreadyActive,
    #[error("OMP ACP prompt authority is not proven read-only")]
    ReadOnlyUnproven,
    #[error("OMP ACP pending request capacity is exhausted")]
    RequestCapacity,
    #[error("OMP ACP record is malformed")]
    MalformedFrame,
    #[error("OMP ACP record exceeded a mechanical limit: {0}")]
    Limit(&'static str),
    #[error("OMP ACP method is unknown or arrived on the wrong path")]
    UnexpectedMethod,
    #[error("OMP ACP request, session, Turn, item, or tool correlation failed")]
    Correlation,
    #[error("OMP ACP activity belongs to a stale Turn intent")]
    StaleIntent,
    #[error("OMP ACP terminal fact conflicts with an existing terminal")]
    TerminalConflict,
    #[error("OMP ACP terminal reason is unknown")]
    UnknownTerminal,
    #[error("OMP ACP remote request failed")]
    RemoteFailure,
    #[error("OMP ACP native path is nonabsolute or contains a lexical alias")]
    NativePath,
    #[error("OMP ACP resource path was foreign, ambiguous, missing, or noncanonical")]
    Resource,
    #[error("OMP ACP tool lifecycle transition is invalid")]
    ToolTransition,
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Wire(#[from] WireError),
}
