//! Pi's wait before retrying a failed reply, shown as a session activity the
//! user can stop: stopping it is Pi's own `abort_retry`.

use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterRuntimeError},
    contract::RequiredText,
    intent::{
        ExternalId,
        session_event::{SessionActivity, SessionActivityState, SessionScopedEvent},
    },
};

use super::super::PiRuntime;

/// Retry activities are named after the Turn whose reply is retried.
const RETRY_PREFIX: &str = "retry:";

impl PiRuntime {
    pub(super) fn retry_activity(
        &self,
        attempt: u64,
        max_attempts: Option<u64>,
        state: SessionActivityState,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        let Some(turn) = self.active_turn.as_ref() else {
            return Ok(AdapterEvent::IgnoredLate);
        };
        let progress = match (state, max_attempts) {
            (SessionActivityState::Running, Some(maximum)) => {
                format!("Attempt {attempt} of {maximum} failed; retrying")
            }
            (SessionActivityState::Running, None) => {
                format!("Attempt {attempt} failed; retrying")
            }
            (SessionActivityState::Completed, _) => format!("Retried after attempt {attempt}"),
            // The row's state already says it stopped.
            (SessionActivityState::Failed | SessionActivityState::Cancelled, _) => String::new(),
        };
        Ok(
            self.session_event(SessionScopedEvent::Activity(SessionActivity {
                id: ExternalId::new(
                    "activityId",
                    format!("{RETRY_PREFIX}{}", turn.external_id.as_str()),
                )
                .map_err(|_| AdapterRuntimeError::Frame)?,
                kind: text("activityKind", "retry".to_owned())?,
                label: text("activityLabel", "Retry after an error".to_owned())?,
                state,
                progress: (!progress.is_empty())
                    .then(|| text("activityProgress", progress))
                    .transpose()?,
                in_previous_turn: false,
                stoppable: state == SessionActivityState::Running,
            })),
        )
    }

    /// Whether `activity` names the running Turn's wait to retry, while Pi
    /// still waits.
    pub(super) fn is_current_retry(&self, activity: &ExternalId) -> bool {
        self.translator.waiting_to_retry()
            && activity
                .as_str()
                .strip_prefix(RETRY_PREFIX)
                .zip(self.active_turn.as_ref())
                .is_some_and(|(turn, active)| turn == active.external_id.as_str())
    }
}

fn text<const N: usize>(
    field: &'static str,
    value: String,
) -> Result<RequiredText<N>, AdapterRuntimeError> {
    RequiredText::new(field, value).map_err(|_| AdapterRuntimeError::Frame)
}
