use serde_json::Value;

use crate::acp::bindings::adapter::RefusalDetail;
#[cfg(test)]
use crate::acp::lifecycle::TurnState;

use super::profile::StandardAcpProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionUpdateKind {
    UserMessageChunk,
    AgentMessageChunk,
    AgentThoughtChunk,
    ToolCall,
    ToolCallUpdate,
    Plan,
    UsageUpdate,
    AvailableCommandsUpdate,
    CurrentModeUpdate,
    ConfigOptionUpdate,
    SessionInfoUpdate,
    AsyncTaskSpawned,
    AsyncTaskProgress,
    AsyncTaskStateUpdate,
    SubagentSpawned,
    SubagentStateUpdate,
}

pub(crate) const SESSION_UPDATE_KINDS: &[SessionUpdateKind] = &[
    SessionUpdateKind::UserMessageChunk,
    SessionUpdateKind::AgentMessageChunk,
    SessionUpdateKind::AgentThoughtChunk,
    SessionUpdateKind::ToolCall,
    SessionUpdateKind::ToolCallUpdate,
    SessionUpdateKind::Plan,
    SessionUpdateKind::UsageUpdate,
    SessionUpdateKind::AvailableCommandsUpdate,
    SessionUpdateKind::CurrentModeUpdate,
    SessionUpdateKind::ConfigOptionUpdate,
    SessionUpdateKind::SessionInfoUpdate,
    SessionUpdateKind::AsyncTaskSpawned,
    SessionUpdateKind::AsyncTaskProgress,
    SessionUpdateKind::AsyncTaskStateUpdate,
    SessionUpdateKind::SubagentSpawned,
    SessionUpdateKind::SubagentStateUpdate,
];

impl SessionUpdateKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        SESSION_UPDATE_KINDS
            .iter()
            .copied()
            .find(|kind| kind.as_wire() == value)
    }

    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            Self::UserMessageChunk => "user_message_chunk",
            Self::AgentMessageChunk => "agent_message_chunk",
            Self::AgentThoughtChunk => "agent_thought_chunk",
            Self::ToolCall => "tool_call",
            Self::ToolCallUpdate => "tool_call_update",
            Self::Plan => "plan",
            Self::UsageUpdate => "usage_update",
            Self::AvailableCommandsUpdate => "available_commands_update",
            Self::CurrentModeUpdate => "current_mode_update",
            Self::ConfigOptionUpdate => "config_option_update",
            Self::SessionInfoUpdate => "session_info_update",
            Self::AsyncTaskSpawned => "async_task_spawned",
            Self::AsyncTaskProgress => "async_task_progress",
            Self::AsyncTaskStateUpdate => "async_task_state_update",
            Self::SubagentSpawned => "subagent_spawned",
            Self::SubagentStateUpdate => "subagent_state_update",
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
}

#[cfg(test)]
pub(crate) const STOP_REASONS: &[StopReason] = &[
    StopReason::EndTurn,
    StopReason::MaxTokens,
    StopReason::MaxTurnRequests,
    StopReason::Refusal,
    StopReason::Cancelled,
];

#[cfg(test)]
impl StopReason {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        STOP_REASONS
            .iter()
            .copied()
            .find(|reason| reason.as_wire() == value)
    }

    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::MaxTokens => "max_tokens",
            Self::MaxTurnRequests => "max_turn_requests",
            Self::Refusal => "refusal",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn turn_state(self) -> TurnState {
        match self {
            Self::EndTurn => TurnState::Completed,
            Self::Cancelled => TurnState::Cancelled,
            Self::MaxTokens | Self::MaxTurnRequests | Self::Refusal => TurnState::Failed,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientRequestMethod {
    RequestPermission,
    ReadTextFile,
    WriteTextFile,
    TerminalCreate,
    TerminalOutput,
    TerminalRelease,
    TerminalWaitForExit,
    TerminalKill,
}

#[cfg(test)]
pub(crate) const CLIENT_REQUEST_METHODS: &[ClientRequestMethod] = &[
    ClientRequestMethod::RequestPermission,
    ClientRequestMethod::ReadTextFile,
    ClientRequestMethod::WriteTextFile,
    ClientRequestMethod::TerminalCreate,
    ClientRequestMethod::TerminalOutput,
    ClientRequestMethod::TerminalRelease,
    ClientRequestMethod::TerminalWaitForExit,
    ClientRequestMethod::TerminalKill,
];

#[cfg(test)]
impl ClientRequestMethod {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        CLIENT_REQUEST_METHODS
            .iter()
            .copied()
            .find(|method| method.as_wire() == value)
    }

    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            Self::RequestPermission => "session/request_permission",
            Self::ReadTextFile => "fs/read_text_file",
            Self::WriteTextFile => "fs/write_text_file",
            Self::TerminalCreate => "terminal/create",
            Self::TerminalOutput => "terminal/output",
            Self::TerminalRelease => "terminal/release",
            Self::TerminalWaitForExit => "terminal/wait_for_exit",
            Self::TerminalKill => "terminal/kill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolCallContentKind {
    Content,
    Diff,
    Terminal,
}

pub(crate) const TOOL_CALL_CONTENT_KINDS: &[ToolCallContentKind] = &[
    ToolCallContentKind::Content,
    ToolCallContentKind::Diff,
    ToolCallContentKind::Terminal,
];

impl ToolCallContentKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        TOOL_CALL_CONTENT_KINDS
            .iter()
            .copied()
            .find(|kind| kind.as_wire() == value)
    }

    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Diff => "diff",
            Self::Terminal => "terminal",
        }
    }
}

pub(crate) const UNKNOWN_UPDATE_REASON: &str = "The harness sent a session update this profile does not describe; \
its raw kind is retained and nothing is inferred from it";

pub(crate) const UNKNOWN_NOTIFICATION_REASON: &str = "The harness sent a notification outside the ACP method set; \
its raw method is retained and nothing is inferred from it";

pub(crate) const HARNESS_RPC_ERROR_CODE: &str = "harness_rpc_error";

const AUTH_DETAIL_CHARS: usize = 512;

pub(crate) fn auth_refusal_detail(
    profile: &StandardAcpProfile,
    frame: &Value,
) -> Option<RefusalDetail> {
    let message = bounded_text(frame.get("error")?.get("message"));
    RefusalDetail::new(message.as_deref(), Some(profile.remediation))
}

fn bounded_text(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty() && text.chars().count() <= AUTH_DETAIL_CHARS).then(|| text.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnmappedUpdate {
    Absorbed,
    Surfaced(&'static str),
}

const STANDARD_UNMAPPED_UPDATES: &[(&str, UnmappedUpdate)] = &[
    ("user_message_chunk", UnmappedUpdate::Absorbed),
    ("available_commands_update", UnmappedUpdate::Absorbed),
    ("session_info_update", UnmappedUpdate::Absorbed),
    (
        "current_mode_update",
        UnmappedUpdate::Surfaced(
            "The harness reported a session mode change Sage does not yet project",
        ),
    ),
];

pub(crate) fn unmapped_update(profile: &StandardAcpProfile, kind: &str) -> UnmappedUpdate {
    profile
        .vendor_updates
        .iter()
        .chain(STANDARD_UNMAPPED_UPDATES)
        .find(|(name, _)| *name == kind)
        .map(|(_, disposition)| *disposition)
        .unwrap_or(UnmappedUpdate::Surfaced(UNKNOWN_UPDATE_REASON))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_auth_refusal_carries_the_harness_message_and_the_profile_remediation() {
        let claude = super::super::profile::profile_for("claude-code").expect("claude profile");
        let detail = auth_refusal_detail(
            claude,
            &json!({"error": {"code": -32000, "message": "authentication required"}}),
        )
        .expect("detail");
        assert_eq!(detail.message(), Some("authentication required"));
        assert_eq!(detail.remediation(), Some(claude.remediation));
    }

    #[test]
    fn an_auth_refusal_without_a_message_still_carries_the_profile_remediation() {
        let claude = super::super::profile::profile_for("claude-code").expect("claude profile");
        let detail =
            auth_refusal_detail(claude, &json!({"error": {"code": -32000}})).expect("detail");
        assert_eq!(detail.message(), None);
        assert_eq!(detail.remediation(), Some(claude.remediation));
    }

    #[test]
    fn a_response_without_an_error_yields_no_refusal_detail() {
        let claude = super::super::profile::profile_for("claude-code").expect("claude profile");
        assert!(auth_refusal_detail(claude, &json!({"result": {"sessionId": "one"}})).is_none());
    }

    #[test]
    fn every_session_update_kind_round_trips_through_its_wire_name() {
        for kind in SESSION_UPDATE_KINDS {
            assert_eq!(SessionUpdateKind::parse(kind.as_wire()), Some(*kind));
        }
        assert_eq!(SESSION_UPDATE_KINDS.len(), 16);
        assert_eq!(SessionUpdateKind::parse("claude_nested_transcript"), None);
    }

    #[test]
    fn every_stop_reason_round_trips_and_maps_to_a_terminal_turn_state() {
        for reason in STOP_REASONS {
            assert_eq!(StopReason::parse(reason.as_wire()), Some(*reason));
            assert!(reason.turn_state().is_terminal());
        }
        assert_eq!(STOP_REASONS.len(), 5);
        assert_eq!(StopReason::parse("exhausted"), None);
    }

    #[test]
    fn every_client_request_method_and_tool_content_kind_round_trips() {
        for method in CLIENT_REQUEST_METHODS {
            assert_eq!(ClientRequestMethod::parse(method.as_wire()), Some(*method));
        }
        assert_eq!(CLIENT_REQUEST_METHODS.len(), 8);
        for kind in TOOL_CALL_CONTENT_KINDS {
            assert_eq!(ToolCallContentKind::parse(kind.as_wire()), Some(*kind));
        }
        assert_eq!(TOOL_CALL_CONTENT_KINDS.len(), 3);
    }

    #[test]
    fn a_profile_vendor_row_explains_its_update_and_an_unknown_kind_falls_back() {
        let claude = super::super::profile::profile_for("claude-code").expect("claude profile");
        assert!(matches!(
            unmapped_update(claude, "subagent_spawned"),
            UnmappedUpdate::Surfaced(reason) if reason != UNKNOWN_UPDATE_REASON
        ));
        assert_eq!(
            unmapped_update(claude, "totally_invented"),
            UnmappedUpdate::Surfaced(UNKNOWN_UPDATE_REASON)
        );
    }

    #[test]
    fn session_metadata_updates_are_absorbed_instead_of_shown_as_activity() {
        let claude = super::super::profile::profile_for("claude-code").expect("claude profile");
        for kind in [
            "available_commands_update",
            "session_info_update",
            "user_message_chunk",
        ] {
            assert_eq!(unmapped_update(claude, kind), UnmappedUpdate::Absorbed);
        }
        assert!(matches!(
            unmapped_update(claude, "current_mode_update"),
            UnmappedUpdate::Surfaced(_)
        ));
    }
}
