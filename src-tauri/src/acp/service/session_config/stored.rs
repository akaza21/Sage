//! A session's config while it has no live runtime.
//!
//! What a live session reports is kept as its snapshot. A parked or
//! restarted chat shows that snapshot with the stored choices as current
//! values, marked as restored, until the resumed runtime reports its own
//! config. A choice made meanwhile is stored the same way a confirmed one
//! is, so the restore reapplies it before the next prompt: the runtime is
//! not started just to change a setting, and the restore stays the one path
//! that brings a resumed session to its stored choices.
use crate::acp::{
    contract::SageAgentSessionId,
    scheduler::RuntimeOwner,
    session_config::{ConfigMutation, SessionConfig, SnapshotRecorder, choices},
};

use super::AcpService;

/// Why a choice for a session without a live runtime was not stored.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QueueError {
    /// Nothing restored offers this mode or value.
    NotOffered,
    Store,
}

impl AcpService {
    pub(in crate::acp::service) fn session_config_snapshot_recorder(&self) -> SnapshotRecorder {
        let database = self.database.clone();
        std::sync::Arc::new(move |owner: &RuntimeOwner, config: &SessionConfig| {
            let saved = database.write_connection().and_then(|connection| {
                choices::save_snapshot(
                    &connection,
                    &owner.session_id,
                    config,
                    crate::persistence::now_millis(),
                )
                .map_err(Into::into)
            });
            if let Err(error) = saved {
                tracing::warn!(?error, "session config snapshot was not stored");
            }
        })
    }

    /// The config a session without a live runtime shows, if any is stored.
    pub(in crate::acp::service) fn restored_session_config(
        &self,
        session: &SageAgentSessionId,
    ) -> Option<SessionConfig> {
        choices::restored(&self.database.connection(), session)
            .inspect_err(|error| tracing::warn!(?error, "stored session config was not read"))
            .ok()
            .flatten()
    }

    /// Stores `choice` for a session that has no live runtime, when what it
    /// shows offers it. Its next runtime reapplies it before the next prompt.
    pub(crate) fn queue_session_config_choice(
        &self,
        session: &SageAgentSessionId,
        choice: &ConfigMutation,
    ) -> Result<(), QueueError> {
        let shown = self
            .restored_session_config(session)
            .ok_or(QueueError::NotOffered)?;
        let offered = match choice {
            ConfigMutation::Mode(mode) => shown.validate_mode(mode.as_str()),
            ConfigMutation::Option { config_id, value } => {
                shown.validate_value(config_id.as_str(), value)
            }
        };
        offered.map_err(|_| QueueError::NotOffered)?;
        let connection = self
            .database
            .write_connection()
            .map_err(|_| QueueError::Store)?;
        match choices::record(
            &connection,
            session,
            choice,
            crate::persistence::now_millis(),
        ) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(QueueError::Store),
        }
    }
}
