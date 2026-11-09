//! Running MCP tool calls of the active Turn, kept so a progress report can
//! restate the call with its latest status line. Codex progress carries only
//! the message, so the call's normalized payload is remembered from its
//! `item/started` until its terminal item.
//!
//! Cap: [`RUNNING_MCP_TOOLS_MAX`] calls. Each holds one running payload,
//! whose label, server and status line are byte-capped by their types and
//! whose result is absent while the call runs. The set is cleared when the
//! Turn ends or the session closes.

use crate::acp::{
    contract::SafeText,
    intent::{NormalizedPayload, TOOL_PROGRESS_BYTES},
};

use super::CodexAdapter;

/// Concurrent MCP calls tracked per Turn; the oldest is forgotten beyond it,
/// so its later progress is ignored like any unknown call's.
pub(crate) const RUNNING_MCP_TOOLS_MAX: usize = 32;

#[derive(Clone, Debug, PartialEq)]
struct RunningMcpTool {
    turn: String,
    item: String,
    payload: NormalizedPayload,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct RunningMcpTools {
    calls: Vec<RunningMcpTool>,
}

impl CodexAdapter {
    /// Remembers a running MCP call, replacing an earlier state of it.
    pub(in crate::acp::bindings::codex) fn track_running_mcp_tool(
        &mut self,
        turn: &str,
        item: &str,
        payload: &NormalizedPayload,
    ) {
        let calls = &mut self.running_mcp_tools.calls;
        calls.retain(|call| call.item != item);
        if calls.len() == RUNNING_MCP_TOOLS_MAX {
            calls.remove(0);
        }
        calls.push(RunningMcpTool {
            turn: turn.to_owned(),
            item: item.to_owned(),
            payload: payload.clone(),
        });
    }

    pub(in crate::acp::bindings::codex) fn forget_running_mcp_tool(&mut self, item: &str) {
        self.running_mcp_tools
            .calls
            .retain(|call| call.item != item);
    }

    pub(crate) fn clear_running_mcp_tools(&mut self) {
        self.running_mcp_tools.calls.clear();
    }

    #[cfg(test)]
    pub(crate) fn running_mcp_tool_count(&self) -> usize {
        self.running_mcp_tools.calls.len()
    }

    /// Sets the status line of a running call and returns its restated
    /// payload, or `None` when the call is unknown, finished, or already
    /// shows this status line.
    pub(in crate::acp::bindings::codex) fn report_mcp_tool_progress(
        &mut self,
        turn: &str,
        item: &str,
        progress: Option<SafeText<TOOL_PROGRESS_BYTES>>,
    ) -> Option<NormalizedPayload> {
        let call = self
            .running_mcp_tools
            .calls
            .iter_mut()
            .find(|call| call.turn == turn && call.item == item)?;
        let NormalizedPayload::ToolCall { execution, .. } = &mut call.payload else {
            return None;
        };
        let progress = progress.map(Box::new);
        if execution.progress == progress {
            return None;
        }
        execution.progress = progress;
        Some(call.payload.clone())
    }
}
