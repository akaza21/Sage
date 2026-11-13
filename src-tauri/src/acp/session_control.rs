//! Requests that act on a running harness session without starting a turn:
//! undoing a turn's file changes, stopping one running activity, naming
//! the session after its chat, and listing, reloading, reconnecting or
//! signing in to its MCP servers.

use super::{contract::SageTurnId, intent::ExternalId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionControl {
    /// Restores the files the harness changed in this turn and every later
    /// one to how they were before the turn.
    UndoTurn { turn_id: SageTurnId },
    /// Stops one running activity the harness reported as stoppable.
    StopActivity { activity_id: ExternalId },
    /// Names the harness's session after its chat, where the harness lists
    /// its sessions by name.
    SetTitle {
        title: super::contract::RequiredText<{ super::intent::session_event::SESSION_TITLE_BYTES }>,
    },
    /// Lists the session's MCP servers and their states.
    ReadMcpServers,
    /// Reloads the MCP server configuration for the session.
    ReloadMcpServers,
    /// Starts signing in to one MCP server; the reply carries where.
    SignInMcpServer {
        name: super::contract::RequiredText<128>,
    },
    /// Connects one MCP server again, without changing its configuration.
    ReconnectMcpServer {
        name: super::contract::RequiredText<128>,
    },
}

impl SessionControl {
    /// Whether a control missing its deadline leaves Sage's state as it
    /// was: the binding stops waiting and drops the late reply, and the
    /// harness may still finish the work. A rename is cosmetic, reading the
    /// MCP servers changes nothing, and reconnecting a server keeps its
    /// configuration.
    pub(crate) fn abandons_at_deadline(&self) -> bool {
        self.answers_at_once() || matches!(self, Self::ReconnectMcpServer { .. })
    }

    /// Whether the harness answers without waiting on outside work, so a
    /// short deadline suffices. Connecting a server waits on that server.
    pub(crate) fn answers_at_once(&self) -> bool {
        matches!(self, Self::SetTitle { .. } | Self::ReadMcpServers)
    }

    pub(crate) fn actual_bytes(&self) -> usize {
        match self {
            Self::UndoTurn { turn_id } => turn_id.as_str().len(),
            Self::StopActivity { activity_id } => activity_id.as_str().len(),
            Self::SetTitle { title } => title.as_str().len(),
            Self::ReadMcpServers | Self::ReloadMcpServers => 0,
            Self::SignInMcpServer { name } | Self::ReconnectMcpServer { name } => {
                name.as_str().len()
            }
        }
    }
}
