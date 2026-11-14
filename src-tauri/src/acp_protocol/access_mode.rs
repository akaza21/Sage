//! The access modes a chat session runs under, and how a harness offers them.
//! Settings store the mode, bindings map it to their own session modes, and
//! the WebView reads both through these serialized forms.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionAccessMode {
    ReadOnly,
    Ask,
    WorkspaceWrite,
    /// File edits are approved without asking; other tools still ask.
    AcceptEdits,
    /// The harness plans and does not change files.
    Plan,
    /// The harness approves actions it judges safe and asks about the rest.
    Auto,
    /// Every action runs without asking. Granted per chat session after an
    /// explicit confirmation; never a stored default.
    FullAccess,
}

impl SessionAccessMode {
    /// Whether this mode can be a binding's stored default for new chats.
    /// A mode that needs a confirmation never can: it would reach every
    /// chat without one.
    pub(crate) fn is_default_eligible(self) -> bool {
        !self.needs_confirmation()
    }

    /// Whether choosing this mode for a chat takes a confirmed token: the
    /// modes that run actions without asking beyond file edits.
    pub(crate) fn needs_confirmation(self) -> bool {
        matches!(self, Self::FullAccess | Self::Auto)
    }
}

/// One access mode as a harness offers it: the composer's menu entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessModeCapability {
    pub(crate) id: SessionAccessMode,
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) fallback: bool,
    /// The harness session mode this access mode selects. The composer offers
    /// the mode only while the live session advertises this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_mode_id: Option<String>,
}
