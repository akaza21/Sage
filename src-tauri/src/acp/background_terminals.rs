use serde::Serialize;

use super::intent::SafeResourceLocation;

/// Most terminals one listing carries; the harness is asked for no more.
pub(crate) const MAX_BACKGROUND_TERMINALS: usize = 32;
pub(crate) const TERMINAL_HANDLE_BYTES: usize = 128;
pub(crate) const TERMINAL_COMMAND_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalAction {
    List,
    Stop(String),
    StopAll,
}

impl TerminalAction {
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Stop(handle) => handle.len() + 1,
            Self::List | Self::StopAll => 1,
        }
    }
}

pub(crate) fn valid_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle.len() <= TERMINAL_HANDLE_BYTES
        && !handle.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackgroundTerminal {
    pub(crate) handle: String,
    /// The sanitized command line.
    pub(crate) command: String,
    pub(crate) cwd: Option<SafeResourceLocation>,
}
