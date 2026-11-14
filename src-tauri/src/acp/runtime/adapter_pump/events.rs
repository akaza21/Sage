use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnScope {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    pub binding_digest: String,
}

/// Owner of a session-scoped fact: the exact session and runtime epoch,
/// with no Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionScope {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub binding_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DurableAdapterEvent {
    Turn {
        scope: TurnScope,
        state: TurnState,
        now_millis: i64,
    },
    Item {
        scope: TurnScope,
        external_item_id: ExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
        payload_digest: String,
        now_millis: i64,
    },
    /// Native child identity stays attached to its collaboration Item through
    /// the journal transaction, never to the renderer-facing payload.
    ChildThreadItem {
        scope: TurnScope,
        external_item_id: ExternalId,
        lifecycle: ItemLifecycle,
        payload: NormalizedPayload,
        payload_digest: String,
        child_thread_id: ExternalId,
        now_millis: i64,
    },
    ChildLiveItem {
        scope: TurnScope,
        child_thread_id: ExternalId,
        child_turn_id: ExternalId,
        item_id: ExternalId,
        role: Option<crate::acp_protocol::codex_protocol::ChildTextRole>,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
        payload_digest: String,
        now_millis: i64,
    },
    ChildTurnStatus {
        scope: TurnScope,
        child_thread_id: ExternalId,
        child_turn_id: ExternalId,
        status: TurnState,
        now_millis: i64,
    },
    /// Bounded old/new bytes for the preceding `FileChangeDiff` item with the
    /// same external key. Applied atomically with its item in the store batch.
    FilePair {
        scope: TurnScope,
        external_item_id: ExternalId,
        old_text: String,
        new_text: String,
        now_millis: i64,
    },
    ActivityItem(Box<DurableActivityItem>),
    PermissionRequest {
        intent: PermissionIntent,
        now_millis: i64,
    },
    /// Legacy paired-denial input retained for recovery characterization.
    /// New ingress normalizes to PermissionRequest and waits for a decision.
    #[allow(
        dead_code,
        reason = "consumers handle this variant; nothing produces it yet"
    )]
    PermissionDeny {
        intent: PermissionIntent,
        now_millis: i64,
    },
    PermissionResolved {
        scope: TurnScope,
        request_id: SageServerRequestId,
        state: ServerRequestState,
        now_millis: i64,
    },
    RuntimePayload {
        scope: TurnScope,
        external_item_id: ExternalId,
        payload: NormalizedPayload,
        payload_digest: String,
        now_millis: i64,
    },
    UsageSnapshot {
        scope: TurnScope,
        input_tokens: u64,
        output_tokens: u64,
        context_window: Option<u32>,
        now_millis: i64,
    },
    TerminalReason {
        scope: TurnScope,
        reason: TurnTerminalReason,
        message: Option<String>,
        remediation: Option<String>,
        now_millis: i64,
    },
    Session {
        scope: SessionScope,
        event: crate::acp::intent::session_event::SessionScopedEvent,
        now_millis: i64,
    },
}
