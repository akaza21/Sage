use crate::acp::session_config::{
    SnapshotRecorder, choices,
    notices::{self, SessionConfigNotice},
};
use sage_acp_protocol::{
    contract::SageAgentSessionId,
    scheduler::RuntimeOwner,
    session_config::{ConfigMutation, SessionConfig},
};
use sage_db::error::SqliteResultExt;

use super::AcpService;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QueueError {
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
                    sage_core::time::now_millis(),
                )
                .db()
            });
            if let Err(error) = saved {
                tracing::warn!(?error, "session config snapshot was not stored");
            }
        })
    }

    pub(in crate::acp::service) fn restored_session_config(
        &self,
        session: &SageAgentSessionId,
    ) -> Option<SessionConfig> {
        choices::restored(&self.database.connection(), session)
            .inspect_err(|error| tracing::warn!(?error, "stored session config was not read"))
            .ok()
            .flatten()
    }

    pub(in crate::acp::service) fn stored_choices_view(
        &self,
        session: &SageAgentSessionId,
    ) -> Option<SessionConfig> {
        choices::stored_only(&self.database.connection(), session)
            .inspect_err(|error| tracing::warn!(?error, "stored session config was not read"))
            .ok()
            .flatten()
    }

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
        let stored = self.database.write_connection().and_then(|mut connection| {
            let transaction = connection.transaction().db()?;
            notices::clear(&transaction, session, choice).db()?;
            let written =
                choices::record(&transaction, session, choice, sage_core::time::now_millis())
                    .db()?;
            transaction.commit().db()?;
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
