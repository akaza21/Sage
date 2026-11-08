use super::*;
use crate::acp::{
    contract::BoundedVec,
    scheduler::RuntimeOwner,
    session_config::{
        ConfigIntent, NoticeSink, SessionConfig,
        choices::{Awaiting, RestoredSessionConfig},
    },
};

mod restore;
#[cfg(test)]
mod restore_race_tests;
#[cfg(test)]
mod restore_tests;
mod stored;
#[cfg(test)]
mod stored_tests;
pub(crate) use stored::QueueError;
#[cfg(test)]
mod tests;

impl AcpService {
    pub(crate) fn set_session_config_notice_sink(&self, sink: NoticeSink) {
        for lane in self.lanes() {
            lane.runtime.config.set_sink(sink.clone());
        }
    }
    pub(crate) fn configure_session(
        &self,
        intent: ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.configure_session_until(intent, None)
    }
    /// `deadline` bounds the harness answer; `None` keeps the host's own.
    pub(super) fn configure_session_until(
        &self,
        intent: ConfigIntent,
        deadline: Option<std::time::Instant>,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.send_config(intent, deadline, false)
    }
    /// A restore change is stored only while it is still the stored value
    /// (a newer pick wins), and one the harness does not answer in time
    /// leaves the runtime running: the restore tries it again.
    pub(super) fn send_config(
        &self,
        intent: ConfigIntent,
        deadline: Option<std::time::Instant>,
        restoring: bool,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let lane = self
            .lane_for(&intent.owner)
            .ok_or(crate::acp::runtime::HostError::Intent)?;
        let host = lane
            .runtime
            .for_owner(&intent.owner)
            .map_err(|_| crate::acp::runtime::HostError::Intent)?;
        lane.runtime
            .config
            .validate(&intent)
            .map_err(|_| crate::acp::runtime::HostError::Intent)?;
        let result = match deadline {
            Some(deadline) => host.configure_session_until(intent.clone(), deadline),
            None => host.configure_session(intent.clone()),
        };
        if result.is_ok() && (!restoring || self.still_stored(&intent.owner, &intent.mutation)) {
            // The harness confirmed this choice: it is what a resumed
            // runtime of this session restores.
            self.remember_config_choice(&intent.owner, &intent.mutation);
        }
        if matches!(
            result,
            Err(crate::acp::runtime::HostError::Unavailable
                | crate::acp::runtime::HostError::Closed
                | crate::acp::runtime::HostError::Deadline
                | crate::acp::runtime::HostError::DeliveryUnknown
                | crate::acp::runtime::HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(intent.owner.clone(), host);
        }
        result.map(|_| ())
    }
    pub(super) fn compose_session_config(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        page.session_config = None;
        let mut sessions = page.sessions.as_slice().to_vec();
        for session in &mut sessions {
            session.capabilities.config_count = 0;
            if Some(&session.id) != page.current_session_id.as_ref()
                || session.residency != crate::acp::contract::Residency::Hot
                || !matches!(
                    session.state,
                    crate::acp::contract::SessionState::Ready
                        | crate::acp::contract::SessionState::Active
                )
            {
                continue;
            }
            let Some(runtime_epoch) = session.runtime_epoch else {
                continue;
            };
            let owner = RuntimeOwner {
                project_id: page.project_id.clone(),
                chat_id: page.chat_id.clone(),
                session_id: session.id.clone(),
                runtime_epoch,
            };
            let Some(lane) = self.lane_for(&owner) else {
                continue;
            };
            if matches!(
                lane.runtime.for_owner(&owner),
                Ok(_) | Err(super::runtime_slot::RuntimeSlotError::Recovery)
            ) {
                page.session_config = lane.runtime.config.snapshot(&owner);
                session.capabilities.config_count = page
                    .session_config
                    .as_ref()
                    .filter(|c| !c.removed)
                    .map_or(0, |c| c.config.config_options.as_slice().len() as u16);
            }
        }
        // A session without a live config shows what it last reported, with
        // its stored choices, until its runtime resumes and reports again. A
        // live runtime that has not reported yet shows them the same way.
        page.restored_session_config = None;
        page.session_config_notice = None;
        let current = sessions
            .iter()
            .find(|session| Some(&session.id) == page.current_session_id.as_ref());
        if page.session_config.is_none()
            && let Some(session) = current.filter(|session| {
                matches!(
                    session.state,
                    crate::acp::contract::SessionState::Ready
                        | crate::acp::contract::SessionState::Active
                )
            })
        {
            let awaiting = if session.runtime_epoch.is_some()
                && session.residency == crate::acp::contract::Residency::Hot
            {
                Awaiting::Report
            } else {
                Awaiting::Resume
            };
            page.restored_session_config =
                self.restored_session_config(&session.id)
                    .map(|config| RestoredSessionConfig {
                        sage_agent_session_id: session.id.clone(),
                        awaiting,
                        config,
                    });
        }
        if let Some(session) = current {
            page.session_config_notice = self.session_config_notice(&session.id);
        }
        page.sessions = BoundedVec::new("sessions", sessions)
            .map_err(|_| StoreError::Corrupt("session config page"))?;
        Ok(())
    }

    /// Keeps the page within its budget. A restored config that does not
    /// fit shows only the stored choices, and none at all if even those do
    /// not fit; the transcript always stays loadable.
    pub(super) fn fit_session_config_to_page_budget(
        &self,
        page: &mut AcpChatPage,
    ) -> Result<(), StoreError> {
        fit_restored_to_page_budget(
            page,
            crate::acp::limits::LIMITS.durable.history_page_bytes as usize,
            |session| self.stored_choices_view(session),
        )
    }
}

fn page_bytes(page: &AcpChatPage) -> Result<usize, StoreError> {
    serde_json::to_vec(page)
        .map(|bytes| bytes.len())
        .map_err(|_| StoreError::Corrupt("session config page"))
}

/// The bytes a page's restored config needs, when the page is over budget
/// with it: history is loaded again leaving that much room.
pub(super) fn restored_reserve(page: &AcpChatPage) -> Result<Option<usize>, StoreError> {
    let Some(restored) = &page.restored_session_config else {
        return Ok(None);
    };
    if page_bytes(page)? <= crate::acp::limits::LIMITS.durable.history_page_bytes as usize {
        return Ok(None);
    }
    serde_json::to_vec(restored)
        .map(|bytes| Some(bytes.len().saturating_add(RESTORED_FIELD_BYTES)))
        .map_err(|_| StoreError::Corrupt("session config page"))
}

/// The `"restoredSessionConfig":` key and separator around the value.
const RESTORED_FIELD_BYTES: usize = 32;

fn fit_restored_to_page_budget(
    page: &mut AcpChatPage,
    limit: usize,
    stored_only: impl FnOnce(&crate::acp::contract::SageAgentSessionId) -> Option<SessionConfig>,
) -> Result<(), StoreError> {
    if page_bytes(page)? <= limit {
        return Ok(());
    }
    if let Some(restored) = page.restored_session_config.take() {
        page.restored_session_config = stored_only(&restored.sage_agent_session_id)
            .map(|config| RestoredSessionConfig { config, ..restored });
        if page_bytes(page)? <= limit {
            return Ok(());
        }
        page.restored_session_config = None;
        if page_bytes(page)? <= limit {
            return Ok(());
        }
    }
    Err(StoreError::Capacity {
        resource: "historyPage",
        limit: crate::acp::limits::LIMITS.durable.history_page_bytes,
    })
}
