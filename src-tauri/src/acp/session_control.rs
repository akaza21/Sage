//! Requests that act on a running harness session without starting a turn:
//! undoing a turn's file changes and stopping one running activity.

use super::{contract::SageTurnId, intent::ExternalId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionControl {
    /// Restores the files the harness changed in this turn and every later
    /// one to how they were before the turn.
    UndoTurn { turn_id: SageTurnId },
    /// Stops one running activity the harness reported as stoppable.
    StopActivity { activity_id: ExternalId },
}

impl SessionControl {
    pub(crate) fn actual_bytes(&self) -> usize {
        match self {
            Self::UndoTurn { turn_id } => turn_id.as_str().len(),
            Self::StopActivity { activity_id } => activity_id.as_str().len(),
        }
    }
}
