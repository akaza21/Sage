//! These requests act on a running harness session without starting a turn.

use super::{contract::SageTurnId, intent::ExternalId};

/// Validated durable turn identity and its position among native user prompts.
/// Bindings interpret their own native IDs; none are supplied by the WebView.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTarget {
    pub turn_id: SageTurnId,
    pub native_turn_id: Option<ExternalId>,
    pub following_messages: u32,
    /// Required for position-based lookup; a durable native ID can stand alone.
    pub prompt_digest: Option<[u8; 32]>,
}

impl ConversationTarget {
    pub fn actual_bytes(&self) -> usize {
        self.turn_id.as_str().len()
            + self
                .native_turn_id
                .as_ref()
                .map_or(0, |id| id.as_str().len())
            + 4
            + self.prompt_digest.map_or(0, |_| 32)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionControl {
    RewindConversation {
        before: ConversationTarget,
    },
    ForkAt {
        target: ConversationTarget,
    },
    /// Restores the files the harness changed in this turn and every later
    /// one to how they were before the turn.
    UndoTurn {
        turn_id: SageTurnId,
    },
    /// Stops one running activity the harness reported as stoppable.
    StopActivity {
        activity_id: ExternalId,
    },
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
    ReconnectMcpServer {
        name: super::contract::RequiredText<128>,
    },
}

impl SessionControl {
    /// The binding may need a bounded native history page to resolve the target.
    pub fn may_read_history(&self) -> bool {
        matches!(self, Self::RewindConversation { .. } | Self::ForkAt { .. })
    }
    /// Whether a control missing its deadline leaves Sage's state as it
    /// was: the binding stops waiting and drops the late reply, and the
    /// harness may still finish the work. A rename is cosmetic, reading the
    /// MCP servers changes nothing, and reconnecting a server keeps its
    /// configuration.
    pub fn abandons_at_deadline(&self) -> bool {
        self.answers_at_once() || matches!(self, Self::ReconnectMcpServer { .. })
    }

    /// Whether the harness answers without waiting on outside work, so a
    /// short deadline suffices. Connecting a server waits on that server.
    pub fn answers_at_once(&self) -> bool {
        matches!(self, Self::SetTitle { .. } | Self::ReadMcpServers)
    }

    pub fn actual_bytes(&self) -> usize {
        match self {
            Self::RewindConversation { before } | Self::ForkAt { target: before } => {
                before.actual_bytes()
            }
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
