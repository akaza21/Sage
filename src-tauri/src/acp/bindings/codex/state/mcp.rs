use serde_json::{Map, Value};

use super::{AdapterError, CodexAdapter, RequestKind};

/// Abandoned list reads whose late answers are still dropped; past these, a
/// new list waits for Codex to answer one of them.
const MAX_ABANDONED_LISTS: usize = 4;
use crate::acp::{
    bindings::codex::request::OutgoingRequest,
    contract::SafeText,
    goal::NativeRpcReply,
    mcp::inventory::{McpAuthUrl, McpInventory},
    session_control::SessionControl,
};

impl CodexAdapter {
    pub(crate) fn mcp_request(
        &mut self,
        id: u64,
        control: &SessionControl,
    ) -> Result<OutgoingRequest, AdapterError> {
        if self.negotiated.is_none() {
            return Err(AdapterError::CapabilityUnavailable);
        }
        // A list Codex is slow to answer holds back only further lists, and
        // only once several are outstanding.
        let lists_held = matches!(control, SessionControl::ReadMcpServers)
            && self.mcp_read_tombstones.len() >= MAX_ABANDONED_LISTS;
        if self.mcp_pending() || lists_held {
            return Err(AdapterError::TurnIntentConflict);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = match control {
            SessionControl::ReadMcpServers => OutgoingRequest::mcp_list(id, &thread)?,
            SessionControl::ReloadMcpServers => OutgoingRequest::mcp_reload(id)?,
            SessionControl::SignInMcpServer { name } => {
                OutgoingRequest::mcp_sign_in(id, &thread, name.as_str())?
            }
            _ => return Err(AdapterError::ForeignRequest),
        };
        self.reserve(request)
    }

    pub(crate) fn mcp_pending(&self) -> bool {
        self.pending.iter().any(|(_, kind)| is_mcp(*kind))
    }

    pub(crate) fn abandon_mcp_read(&mut self) {
        let read = self
            .pending
            .iter()
            .find(|(_, kind)| *kind == RequestKind::McpList)
            .map(|(id, _)| *id);
        if let Some(id) = read {
            self.take_pending(id);
            self.mcp_read_tombstones.push(id);
        }
    }

    /// An MCP list answer can exceed the frame bound because Codex includes
    /// every tool's schema.
    pub(super) fn mcp_list_awaited(&self) -> bool {
        !self.mcp_read_tombstones.is_empty() || self.awaits(RequestKind::McpList)
    }

    /// A list answer past the frame bound: `true` when it was the awaited
    /// list, which is then over. An abandoned one is simply dropped.
    pub(crate) fn mcp_list_oversized(&mut self, id: u64) -> bool {
        if self.take_mcp_tombstone(id) {
            return false;
        }
        if self.pending_kind(id) != Some(RequestKind::McpList) {
            return false;
        }
        self.take_pending(id);
        true
    }

    pub(crate) fn take_mcp_tombstone(&mut self, id: u64) -> bool {
        let before = self.mcp_read_tombstones.len();
        self.mcp_read_tombstones.retain(|dead| *dead != id);
        self.mcp_read_tombstones.len() != before
    }

    pub(crate) fn complete_mcp(
        &mut self,
        id: u64,
        kind: RequestKind,
        response: &Map<String, Value>,
    ) -> NativeRpcReply {
        let removed = self.take_pending(id);
        debug_assert_eq!(removed, Some(kind));
        if let Some(error) = response.get("error") {
            return NativeRpcReply::ControlRefused(reason(error));
        }
        let result = response.get("result").and_then(Value::as_object);
        match kind {
            RequestKind::McpList => result
                .and_then(McpInventory::from_codex)
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::McpServers),
            RequestKind::McpSignIn => result
                .and_then(|result| result.get("authorizationUrl"))
                .and_then(Value::as_str)
                .and_then(McpAuthUrl::parse)
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::McpSignIn),
            _ => NativeRpcReply::Controlled(None),
        }
    }
}

fn is_mcp(kind: RequestKind) -> bool {
    matches!(
        kind,
        RequestKind::McpList | RequestKind::McpReload | RequestKind::McpSignIn
    )
}

fn reason(error: &Value) -> Option<SafeText<512>> {
    let message = error.get("message").and_then(Value::as_str)?;
    let mut line: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut end = line.len().min(512);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line.truncate(end);
    SafeText::new("controlSummary", line.trim().to_owned()).ok()
}
