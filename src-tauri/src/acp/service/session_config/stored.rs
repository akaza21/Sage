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
    session_config::{
        ConfigMutation, SessionConfig, SnapshotRecorder, choices,
        notices::{self, SessionConfigNotice},
    },
};

use super::AcpService;

/// Why a choice for a session without a live runtime was not stored.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QueueError {
    /// Nothing restored offers this mode or value.
    NotOffered,
    /// The session's runtime reports its config: change it there.
    Live,
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

    /// Only the session's stored choices, for a page with no room for more.
    pub(in crate::acp::service) fn stored_choices_view(
        &self,
        session: &SageAgentSessionId,
    ) -> Option<SessionConfig> {
        choices::stored_only(&self.database.connection(), session)
            .inspect_err(|error| tracing::warn!(?error, "stored session config was not read"))
            .ok()
            .flatten()
    }

    /// What the composer says about a stored choice the session could not
    /// take back.
    pub(in crate::acp::service) fn session_config_notice(
        &self,
        session: &SageAgentSessionId,
    ) -> Option<SessionConfigNotice> {
        notices::load(&self.database.connection(), session)
            .inspect_err(|error| tracing::warn!(?error, "session config notice was not read"))
            .ok()
            .flatten()
    }

    /// Stores `choice` for a session that has no live config, when what it
    /// shows offers it: a parked session's next runtime reapplies it before
    /// the next prompt, and a live runtime that has not reported its config
    /// yet reapplies it once it has, before the next prompt too.
    pub(crate) fn queue_session_config_choice(
        &self,
        session: &SageAgentSessionId,
        runtime: Option<&RuntimeOwner>,
        choice: &ConfigMutation,
    ) -> Result<(), QueueError> {
        let lane = runtime.and_then(|owner| self.lane_for(owner).zip(Some(owner)));
        // A runtime that reports its config takes changes through it.
        if lane
            .as_ref()
            .is_some_and(|(lane, owner)| lane.runtime.config.live_snapshot(owner).is_some())
        {
            return Err(QueueError::Live);
        }
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
        // A restore that already read the stored choices runs again, and
        // until it reaches this one, what the harness reports for the same
        // option does not replace it.
        if let Some((lane, owner)) = &lane {
            lane.runtime
                .config
                .rearm_restore(owner, std::slice::from_ref(choice));
        }
        // The pick settles any notice about its option, in the same write.
        let stored = self.database.write_connection().and_then(|mut connection| {
            let transaction = connection.transaction()?;
            notices::clear(&transaction, session, choice)?;
            let written = choices::record(
                &transaction,
                session,
                choice,
                crate::persistence::now_millis(),
            )?;
            transaction.commit()?;
            Ok(written)
        });
        // On failure the rearmed restore resets what it holds from what is
        // stored when it runs.
        match stored {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(QueueError::Store),
        }
    }
}
