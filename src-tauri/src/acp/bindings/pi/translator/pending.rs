//! The commands Sage has sent Pi and is waiting on, and the bounded record of
//! answered ones that tells a repeated reply from a stray.
use crate::acp::bindings::pi::protocol::NativeCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingKind {
    Configuration,
    ConfigurationModels,
    State,
    Models,
    SetModel,
    ThinkingLevels,
    Commands,
    SetThinkingLevel,
    SetAutoCompaction,
    NewSession,
    Resume,
    Prompt,
    Compact,
    Steer,
    ClearQueue,
    Abort,
    Messages,
    Entries,
    Native(NativeCommand),
}

impl PendingKind {
    pub(super) const fn starts_turn(self) -> bool {
        matches!(self, Self::Prompt | Self::Compact)
    }

    pub(super) const fn is_config(self) -> bool {
        matches!(
            self,
            Self::State
                | Self::SetModel
                | Self::ThinkingLevels
                | Self::SetThinkingLevel
                | Self::SetAutoCompaction
        )
    }

    /// Reads whose reply may be large: a session's state, catalogue or history.
    pub(super) const fn awaits_large_reply(self) -> bool {
        !matches!(
            self,
            Self::Prompt
                | Self::Compact
                | Self::Steer
                | Self::ClearQueue
                | Self::Abort
                | Self::SetModel
                | Self::SetThinkingLevel
                | Self::SetAutoCompaction
                | Self::Native(_)
        )
    }

    pub(super) const fn command(self) -> &'static str {
        match self {
            Self::Configuration => "get_state",
            Self::ConfigurationModels => "get_available_models",
            Self::State => "get_state",
            Self::Models => "get_available_models",
            Self::SetModel => "set_model",
            Self::Commands => "get_commands",
            Self::ThinkingLevels => "get_available_thinking_levels",
            Self::SetThinkingLevel => "set_thinking_level",
            Self::SetAutoCompaction => "set_auto_compaction",
            // Pi opens a fresh session at launch and Sage runs one session
            // per process, so a new chat takes that session by its id.
            Self::NewSession => "get_state",
            Self::Resume => "switch_session",
            Self::Prompt => "prompt",
            Self::Compact => "compact",
            Self::Steer => "steer",
            Self::ClearQueue => "clear_queue",
            Self::Abort => "abort",
            Self::Messages => "get_messages",
            Self::Entries => "get_entries",
            Self::Native(command) => command.wire(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PendingRequest {
    pub(super) kind: PendingKind,
    // Ownership survives settlement and replacement of the active turn, within
    // the same 16-entry pending bound as every other command.
    pub(super) owner: Option<u64>,
    /// The extension-message marker a steer took; it is given back if the
    /// steer never reaches Pi.
    pub(super) claimed_echo: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletedResponse {
    pub(super) command: String,
    pub(super) success: bool,
    pub(super) payload_fingerprint: u64,
}
