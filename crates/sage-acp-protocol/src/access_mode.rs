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
    /// A mode requiring confirmation cannot be a stored default: it would reach every new chat without that confirmation.
    pub fn is_default_eligible(self) -> bool {
        !self.needs_confirmation()
    }

    /// Modes that run actions without asking beyond file edits require a confirmed token.
    pub fn needs_confirmation(self) -> bool {
        matches!(self, Self::FullAccess | Self::Auto)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessModeCapability {
    pub id: SessionAccessMode,
    pub label: String,
    pub description: String,
    pub fallback: bool,
    /// The harness session mode this access mode selects. The composer offers
    /// the mode only while the live session advertises this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode_id: Option<String>,
}
