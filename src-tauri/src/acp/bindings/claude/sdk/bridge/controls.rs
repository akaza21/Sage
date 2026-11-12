//! Session controls as SDK control requests: undoing a turn rewinds the
//! files Claude Code checkpointed since that turn's message, stopping a
//! task ends one background task, and `/mcp` lists the session's MCP
//! servers or connects one again.

use serde_json::{Value, json};

use super::{
    BridgeError, Inbound, Pending, SdkBridge,
    notices::{TASK_PREFIX, prose},
};
use crate::acp::{
    contract::SafeText, goal::NativeRpcReply, mcp::inventory::McpInventory,
    session_control::SessionControl,
};

/// MCP requests Claude Code has not answered after their callers gave up;
/// at this many, further MCP requests wait until one is answered.
const MAX_ABANDONED_REQUESTS: usize = 4;

/// An MCP request that changes nothing Sage holds, so its caller may give
/// up on it (see `SessionControl::abandons_at_deadline`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum McpRequest {
    /// A read of the session's MCP servers.
    List,
    /// Connecting one MCP server again.
    Reconnect,
}

impl McpRequest {
    /// Claude Code's reply, as the request's native answer.
    pub(super) fn answered(self, reply: Result<&Value, &str>, inbound: &mut Inbound) {
        match (self, reply) {
            (Self::List, Ok(reply)) => SdkBridge::mcp_listed(reply, inbound),
            (Self::List | Self::Reconnect, reply) => SdkBridge::controlled(reply, inbound),
        }
    }
}

impl SdkBridge {
    /// The control request for a session control; one at a time.
    pub(in crate::acp::bindings::claude::sdk) fn session_control(
        &mut self,
        control: &SessionControl,
    ) -> Result<Value, BridgeError> {
        if self.controlling() || (control.abandons_at_deadline() && self.abandoned_full()) {
            return Err(BridgeError::Capacity);
        }
        let mut pending = Pending::Control;
        let request = match control {
            SessionControl::UndoTurn { turn_id } => json!({
                "subtype": "rewind_files",
                "user_message_id": message_uuid(turn_id.as_str()).ok_or(BridgeError::Unsupported)?,
            }),
            SessionControl::StopActivity { activity_id } => {
                let task = activity_id
                    .as_str()
                    .strip_prefix(TASK_PREFIX)
                    .ok_or(BridgeError::Unsupported)?;
                json!({"subtype": "stop_task", "task_id": task})
            }
            SessionControl::ReadMcpServers => {
                pending = Pending::Mcp(McpRequest::List);
                json!({"subtype": "mcp_status"})
            }
            SessionControl::ReconnectMcpServer { name } => {
                pending = Pending::Mcp(McpRequest::Reconnect);
                json!({"subtype": "mcp_reconnect", "serverName": name.as_str()})
            }
            // Claude Code reloads its MCP configuration only together with
            // its plugins, and signs in to servers only in its own `/mcp`.
            SessionControl::SetTitle { .. }
            | SessionControl::ReloadMcpServers
            | SessionControl::SignInMcpServer { .. } => return Err(BridgeError::Unsupported),
        };
        self.control(request, pending)
    }

    /// Enough abandoned requests are still unanswered that no more are sent.
    fn abandoned_full(&self) -> bool {
        self.pending
            .values()
            .filter(|pending| matches!(pending, Pending::Abandoned))
            .count()
            >= MAX_ABANDONED_REQUESTS
    }

    /// A session control is waiting on Claude Code's reply.
    pub(in crate::acp::bindings::claude::sdk) fn controlling(&self) -> bool {
        self.pending.values().any(|pending| {
            matches!(
                pending,
                Pending::Control | Pending::Mcp(_) | Pending::Fork(_)
            )
        })
    }

    /// Stops waiting for an MCP list or reconnect whose caller gave up.
    /// Neither changes what Sage holds, and Claude Code may still finish a
    /// reconnect. The request id stays known, so the late reply is dropped
    /// rather than read as a frame Sage never asked for.
    pub(in crate::acp::bindings::claude::sdk) fn abandon_mcp_requests(&mut self) {
        for pending in self.pending.values_mut() {
            if matches!(pending, Pending::Mcp(_)) {
                *pending = Pending::Abandoned;
            }
        }
    }

    /// Claude Code's MCP servers; a reply without a list is refused.
    fn mcp_listed(reply: &Value, inbound: &mut Inbound) {
        inbound.native = Some(
            McpInventory::from_claude(reply)
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::McpServers),
        );
    }

    /// A fork answers once Claude Code shows it is still running; the copy
    /// itself is made when the fork's chat first launches (see `fork`).
    pub(in crate::acp::bindings::claude::sdk) fn fork(
        &mut self,
        config: Option<std::path::PathBuf>,
    ) -> Result<Value, BridgeError> {
        if self.controlling() || self.prompt.is_some() {
            return Err(BridgeError::Capacity);
        }
        self.control(json!({"subtype": "get_settings"}), Pending::Fork(config))
    }

    /// The fork through the session's latest saved reply, or a refusal
    /// when there is none (or Claude Code failed the round trip).
    pub(super) fn forked(&self, config: Option<&std::path::Path>, inbound: &mut Inbound) {
        inbound.native = Some(
            config
                .and_then(|config| super::super::fork::Fork::of(&self.session_id, config))
                .and_then(|fork| {
                    crate::acp::intent::ExternalId::new("externalSessionId", fork.id()).ok()
                })
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::Forked),
        );
    }

    /// Claude Code's reply to a session control. A rewind it could not
    /// carry out is refused with its reason.
    pub(super) fn controlled(reply: Result<&Value, &str>, inbound: &mut Inbound) {
        let summary = |text: Option<String>| {
            text.and_then(|text| prose(&text, 512))
                .and_then(|text| SafeText::new("controlSummary", text).ok())
        };
        inbound.native = Some(match reply {
            Ok(reply) if reply.get("canRewind").and_then(Value::as_bool) == Some(false) => {
                let reason = reply
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                NativeRpcReply::ControlRefused(summary(reason))
            }
            Ok(reply) => NativeRpcReply::Controlled(summary(
                reply
                    .get("filesChanged")
                    .and_then(Value::as_array)
                    .map(|files| match files.len() {
                        1 => "Restored 1 file".to_owned(),
                        count => format!("Restored {count} files"),
                    }),
            )),
            Err(reason) => NativeRpcReply::ControlRefused(summary(Some(reason.to_owned()))),
        });
    }
}

/// The SDK message id Sage gives a turn's prompt: the UUID in its turn id,
/// so an undo still finds it after the session is resumed.
pub(in crate::acp::bindings::claude::sdk) fn message_uuid(turn_id: &str) -> Option<String> {
    let id = turn_id.strip_prefix("turn-")?;
    uuid::Uuid::parse_str(id).ok().map(|id| id.to_string())
}
