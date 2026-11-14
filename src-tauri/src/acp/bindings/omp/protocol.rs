use sage_acp_protocol::{
    intent::{ExternalId, NormalizedPayload},
    lifecycle::{ItemLifecycle, ToolState},
};

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Operation {
    Initialize,
    NewSession {
        native_cwd: String,
        root_count: usize,
    },
    LoadSession {
        external_session_id: String,
        native_cwd: String,
        root_count: usize,
    },
    ResumeSession,
    Prompt {
        turn_key: String,
        message: String,
    },
    Cancel,
    CloseSession,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Outbound {
    pub request_id: Option<String>,
    pub line: String,
}

impl std::fmt::Debug for Outbound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Outbound")
            .field("request_id", &self.request_id)
            .field("line", &"[redacted ACP frame]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSummary {
    pub external_id: ExternalId,
    pub name: String,
    pub is_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptTerminal {
    Completed,
    Cancelled,
    Failed,
}

pub(crate) use crate::acp::bindings::adapter::ItemUpdateMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdapterEvent {
    Initialized {
        protocol_version: u64,
        load_session: bool,
    },
    SessionReady {
        external_session_id: ExternalId,
        models: Vec<ModelSummary>,
    },
    TurnRunning {
        turn_key: String,
    },
    Item {
        external_key: ExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
        update_mode: ItemUpdateMode,
    },
    /// Bounded old/new bytes for the preceding `FileChangeDiff` item with the
    /// same external key. Stored as BLOBs; never serialized into the payload.
    FilePair {
        external_key: ExternalId,
        old_text: String,
        new_text: String,
        update_mode: ItemUpdateMode,
    },
    PermissionDeclinePending {
        request_id: ExternalId,
        tool_call_id: ExternalId,
    },
    ToolAuthorityDenied {
        tool_call_id: ExternalId,
    },
    CancelDispatchRequired,
    TurnTerminal {
        turn_key: String,
        terminal: PromptTerminal,
    },
    IgnoredDuplicate,
    IgnoredLate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionResponse {
    pub outbound: Outbound,
    pub event: AdapterEvent,
}
